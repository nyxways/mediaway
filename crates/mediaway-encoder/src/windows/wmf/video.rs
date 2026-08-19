//! Video encode sessions: sync/soft MFT (CPU) or hardware MFT (DX11 Zero-Copy).
//!
//! The MFT may emit B-frame streams in **decode order** while stamping each
//! output sample with its display `pts`, so emitted packets get `dts` =
//! decode-order position (1 tick per frame in the config timebase) and
//! `cto = pts - dts` — never `dts = pts` (see [`Self::drain_output`]).

#![allow(unsafe_code)]

use std::collections::VecDeque;

use crate::{EncodeError, VideoEncoder, VideoEncoderConfig, VideoInputPreference};
use mediaway_common::{
    Bytes, CodecKind, GpuBufferHandle, GpuDeviceHandle, Packet, PixelFormat, StreamInfo,
    VideoFrame, VideoFrameStorage, VideoGeometry,
};
use windows::Win32::Media::MediaFoundation::{
    CLSID_MSH264EncoderMFT, IMFMediaBuffer, IMFSample, IMFTransform, MF_MT_MPEG_SEQUENCE_HEADER,
    MFCreateMemoryBuffer, MFCreateSample, MFT_MESSAGE_COMMAND_DRAIN,
    MFT_MESSAGE_NOTIFY_END_OF_STREAM,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};

use super::codec::{is_supported_video_codec, video_subtype};
use super::dx11::{self, Dx11Session};
use super::runtime::to_hns;
use super::shared::{
    Drain, begin_streaming, bitrate_and_fps, configure_types, nv12_size, output_buffer_hint,
    process_one_output,
};

/// Video encode session (CPU upload or DX11 Zero-Copy) for H.264 / HEVC / AV1 / VP9.
pub(crate) struct WmfVideoEncoder {
    transform: IMFTransform,
    info: StreamInfo,
    time_base_num: u64,
    time_base_den: u32,
    width: u32,
    height: u32,
    nv12_bytes: usize,
    output_buf_size: u32,
    pending: VecDeque<Packet>,
    flushed: bool,
    dx11: Option<Dx11Session>,
    /// Decode-order position (media-timebase ticks) for the next emitted
    /// packet — the MFT may reorder B-frames, so `dts` cannot be the sample's
    /// display `pts` (see [`Self::drain_output`]).
    dts_counter: i64,
}

impl WmfVideoEncoder {
    /// Open according to [`VideoEncoderConfig::input`].
    pub(crate) fn open(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        validate_common(config)?;
        ensure_mf_runtime()?;
        match config.input {
            VideoInputPreference::CpuUploadOk => Self::open_cpu(config),
            VideoInputPreference::ZeroCopyGpu => Self::open_dx11(config),
        }
    }

    fn open_cpu(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        let output_subtype = video_subtype(config.codec)?;
        let transform: IMFTransform = if config.codec == CodecKind::H264 {
            // SAFETY: inbox sync H.264 MFT.
            unsafe { CoCreateInstance(&CLSID_MSH264EncoderMFT, None, CLSCTX_INPROC_SERVER) }
                .map_err(|_| EncodeError::Backend)?
        } else {
            // HEVC/AV1/VP9: no inbox soft encoder on most SKUs — enumerate any MFT.
            dx11::activate_encoder_mft(&output_subtype, false)?
        };

        let (bitrate, fps_num, fps_den) = bitrate_and_fps(
            config.bitrate_bps,
            config.time_base.num,
            config.time_base.den,
        );
        configure_types(
            &transform,
            config.width,
            config.height,
            fps_num,
            fps_den,
            bitrate,
            &output_subtype,
            config.pixel_format,
        )?;
        begin_streaming(&transform)?;
        let output_buf_size = output_buffer_hint(&transform)?;
        let nv12_bytes = nv12_size(config.width, config.height)?;

        let mut enc = Self {
            transform,
            info: stream_info_from(config),
            time_base_num: config.time_base.num,
            time_base_den: config.time_base.den,
            width: config.width,
            height: config.height,
            nv12_bytes,
            output_buf_size,
            pending: VecDeque::new(),
            flushed: false,
            dx11: None,
            dts_counter: 0,
        };
        enc.refresh_extradata();
        Ok(enc)
    }

    fn open_dx11(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        let output_subtype = video_subtype(config.codec)?;
        let Some(GpuDeviceHandle::DirectX11(handle)) = config.gpu_device else {
            return Err(EncodeError::InvalidInput);
        };
        let device = dx11::device_from_handle(handle)?;
        let (transform, session, output_buf_size) = dx11::open_hw_encoder(
            device,
            config.width,
            config.height,
            config.time_base.num,
            config.time_base.den,
            config.bitrate_bps,
            &output_subtype,
            config.pixel_format,
        )?;
        let nv12_bytes = nv12_size(config.width, config.height)?;
        let mut enc = Self {
            transform,
            info: stream_info_from(config),
            time_base_num: config.time_base.num,
            time_base_den: config.time_base.den,
            width: config.width,
            height: config.height,
            nv12_bytes,
            output_buf_size,
            pending: VecDeque::new(),
            flushed: false,
            dx11: Some(session),
            dts_counter: 0,
        };
        enc.refresh_extradata();
        Ok(enc)
    }

    fn refresh_extradata(&mut self) {
        let Ok(mt) = (unsafe { self.transform.GetOutputCurrentType(0) }) else {
            return;
        };
        let Ok(blob_size) = (unsafe { mt.GetBlobSize(&MF_MT_MPEG_SEQUENCE_HEADER) }) else {
            return;
        };
        if blob_size == 0 {
            return;
        }
        let mut buf = vec![0u8; blob_size as usize];
        let mut written = 0u32;
        if unsafe {
            mt.GetBlob(
                &MF_MT_MPEG_SEQUENCE_HEADER,
                &mut buf,
                Some(std::ptr::from_mut(&mut written)),
            )
        }
        .is_ok()
            && written > 0
        {
            buf.truncate(written as usize);
            let codec = self.info.codec();
            if let StreamInfo::Video { extra_data, .. } = &mut self.info {
                // WMF's MF_MT_MPEG_SEQUENCE_HEADER shape depends on the codec: H.264 hands
                // back an Annex-B SPS/PPS blob (container contract wants a full
                // AVCDecoderConfigurationRecord/avcC); AV1 hands back a raw OBU stream
                // (container contract wants a real AV1CodecConfigurationRecord/av1C, see
                // ADR-0010). HEVC/VP9 keep the pre-existing raw-bytes-verbatim fallback —
                // their own config-record correctness is a known, separately-tracked gap
                // (ADR-0010's Decision), not fixed here.
                *extra_data = match codec {
                    CodecKind::H264 => iso_bmff::bitstream::avc::to_avcc(&buf)
                        .avcc
                        .unwrap_or_else(|| Bytes::from(buf)),
                    CodecKind::Av1 => iso_bmff::bitstream::av1::to_av1c(&buf)
                        .av1c
                        .unwrap_or_else(|| Bytes::from(buf)),
                    _ => Bytes::from(buf),
                };
            }
        }
    }

    /// Copy CPU NV12 into an `IMFMediaBuffer` (`upload_cpu_nv12`) — not Zero-Copy.
    fn upload_cpu_nv12(&self, frame: &VideoFrame) -> Result<IMFSample, EncodeError> {
        let VideoFrameStorage::Cpu { data } = &frame.storage else {
            return Err(EncodeError::Unsupported);
        };
        if frame.width != self.width || frame.height != self.height {
            return Err(EncodeError::InvalidInput);
        }
        if data.len() < self.nv12_bytes {
            return Err(EncodeError::InvalidInput);
        }
        let nv12_len = u32::try_from(self.nv12_bytes).map_err(|_| EncodeError::InvalidInput)?;
        let sample: IMFSample = unsafe { MFCreateSample() }.map_err(|_| EncodeError::Backend)?;
        let buffer: IMFMediaBuffer =
            unsafe { MFCreateMemoryBuffer(nv12_len) }.map_err(|_| EncodeError::Backend)?;
        unsafe {
            let mut ptr = std::ptr::null_mut();
            let mut max_len = 0u32;
            let mut cur_len = 0u32;
            buffer
                .Lock(
                    &raw mut ptr,
                    Some(std::ptr::from_mut(&mut max_len)),
                    Some(std::ptr::from_mut(&mut cur_len)),
                )
                .map_err(|_| EncodeError::Backend)?;
            if ptr.is_null() || max_len < nv12_len {
                let _: windows::core::Result<()> = buffer.Unlock();
                return Err(EncodeError::Backend);
            }
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, self.nv12_bytes);
            buffer
                .SetCurrentLength(nv12_len)
                .map_err(|_| EncodeError::Backend)?;
            buffer.Unlock().map_err(|_| EncodeError::Backend)?;
        }
        unsafe { sample.AddBuffer(&buffer) }.map_err(|_| EncodeError::Backend)?;
        let hns = to_hns(frame.pts, self.time_base_num, self.time_base_den);
        let dur = to_hns(
            i64::try_from(frame.duration).unwrap_or(0),
            self.time_base_num,
            self.time_base_den,
        )
        .max(1);
        unsafe {
            sample
                .SetSampleTime(hns)
                .map_err(|_| EncodeError::Backend)?;
            sample
                .SetSampleDuration(dur)
                .map_err(|_| EncodeError::Backend)?;
        }
        Ok(sample)
    }

    fn push_dx11_frame(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        let VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 {
            texture,
            subresource,
        }) = &frame.storage
        else {
            return Err(EncodeError::Unsupported);
        };
        if frame.width != self.width || frame.height != self.height {
            return Err(EncodeError::InvalidInput);
        }
        if let Some(session) = self.dx11.as_mut() {
            dx11::drain_events_nonblocking(session)?;
            dx11::wait_need_input(session)?;
        }
        let sample = dx11::sample_from_dx11_texture(
            *texture,
            *subresource,
            frame.pts,
            frame.duration,
            self.time_base_num,
            self.time_base_den,
        )?;
        unsafe { self.transform.ProcessInput(0, &sample, 0) }.map_err(|_| EncodeError::Backend)?;
        if let Some(session) = self.dx11.as_mut() {
            dx11::consume_need_input(session);
            dx11::drain_events_nonblocking(session)?;
        }
        self.drain_output()?;
        Ok(())
    }

    fn drain_output(&mut self) -> Result<(), EncodeError> {
        let provides = self
            .dx11
            .as_ref()
            .is_some_and(|s| s.output_provides_samples);
        loop {
            if let Some(session) = self.dx11.as_mut() {
                dx11::drain_events_nonblocking(session)?;
            }
            match process_one_output(&self.transform, self.output_buf_size, provides, &self.info)? {
                // The MFT emits B-frame streams in decode order while each
                // output sample carries its *display* timestamp (`pts`). `dts`
                // must be the decode-order position (1 tick per emitted frame,
                // CBR pacing in the config timebase) or muxers would compute
                // wrong durations and `cto = pts - dts`.
                Drain::Packet(mut p) => {
                    p.dts = self.dts_counter;
                    self.dts_counter = self.dts_counter.saturating_add(1);
                    self.pending.push_back(p);
                }
                Drain::NeedMore => break,
                Drain::StreamChange => self.refresh_extradata(),
            }
        }
        Ok(())
    }
}

impl VideoEncoder for WmfVideoEncoder {
    fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    fn push_frame(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        if self.flushed {
            return Err(EncodeError::Closed);
        }
        if self.dx11.is_some() {
            self.push_dx11_frame(frame)?;
        } else {
            let sample = self.upload_cpu_nv12(frame)?;
            unsafe { self.transform.ProcessInput(0, &sample, 0) }
                .map_err(|_| EncodeError::Backend)?;
            self.drain_output()?;
        }
        if self.info.extra_data().is_empty() {
            self.refresh_extradata();
        }
        Ok(())
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        Ok(self.pending.pop_front())
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        if self.flushed {
            return Ok(());
        }
        self.flushed = true;
        unsafe {
            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)
                .map_err(|_| EncodeError::Backend)?;
            self.transform
                .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)
                .map_err(|_| EncodeError::Backend)?;
        }
        self.drain_output()?;
        super::shared::notify_end_streaming(&self.transform);
        Ok(())
    }
}

fn validate_common(config: &VideoEncoderConfig) -> Result<(), EncodeError> {
    if !is_supported_video_codec(config.codec) {
        return Err(EncodeError::Unsupported);
    }
    if config.width == 0
        || config.height == 0
        || !config.width.is_multiple_of(2)
        || !config.height.is_multiple_of(2)
    {
        return Err(EncodeError::InvalidInput);
    }
    if config.pixel_format != PixelFormat::Nv12 && config.pixel_format != PixelFormat::Bgra8 {
        return Err(EncodeError::Unsupported);
    }
    if config.pixel_format == PixelFormat::Bgra8
        && !matches!(config.input, VideoInputPreference::ZeroCopyGpu)
    {
        // CPU upload path is NV12-only (`upload_cpu_nv12`).
        return Err(EncodeError::Unsupported);
    }
    if config.time_base.den == 0 {
        return Err(EncodeError::InvalidInput);
    }
    Ok(())
}

#[allow(clippy::missing_const_for_fn, reason = "StreamInfo holds Bytes")]
fn stream_info_from(config: &VideoEncoderConfig) -> StreamInfo {
    StreamInfo::Video {
        id: 0,
        codec: config.codec,
        time_base: config.time_base,
        geometry: VideoGeometry {
            width: config.width,
            height: config.height,
        },
        extra_data: Bytes::new(),
    }
}

fn ensure_mf_runtime() -> Result<(), EncodeError> {
    super::runtime::ensure_mf()
}

#[cfg(test)]
#[path = "video_tests.rs"]
mod tests;
