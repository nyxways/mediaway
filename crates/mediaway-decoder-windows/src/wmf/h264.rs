//! H.264 decode session: hardware MFT (DX11 Zero-Copy output).

#![allow(unsafe_code)]

use std::collections::VecDeque;

use iso_bmff::bitstream::avc::{annex_b_sequence_header, parse_avc_decoder_config};
use mediaway_common::{
    Bytes, GpuBufferHandle, GpuDeviceHandle, NativeHandle, Packet, PixelFormat, StreamInfo,
    VideoFrame, VideoFrameStorage, VideoGeometry,
};
use mediaway_decoder::{DecodeError, VideoDecoder, VideoDecoderConfig, VideoOutputPreference};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Media::MediaFoundation::{
    IMFSample, IMFTransform, MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_NOTIFY_END_OF_STREAM,
};
use windows::core::Interface;

use super::codec::{is_supported_video_codec, video_subtype};
use super::cpu::{self, open_sw_decoder};
use super::dx11::{self, Dx11Session};
use super::runtime::from_hns;
use super::shared::{
    Drain, begin_streaming, configure_decode_types, notify_end_streaming, output_buffer_size,
    packet_to_sample, process_one_output, read_output_dimensions,
};

/// Keeps DXGI output sample + texture alive until the surface is recycled.
struct GpuFrameHold {
    _sample: IMFSample,
    texture: ID3D11Texture2D,
    subresource: u32,
    pts: i64,
    duration: u64,
    width: u32,
    height: u32,
}

impl GpuFrameHold {
    /// # Errors
    ///
    /// Returns [`DecodeError::Backend`] if the live output texture's COM pointer
    /// is somehow null (not expected in practice — a valid `Interface` value is
    /// never backed by a null vtable).
    fn to_video_frame(&self) -> Result<VideoFrame, DecodeError> {
        let texture = NativeHandle::new(Interface::as_raw(&self.texture) as usize)
            .ok_or(DecodeError::Backend)?;
        Ok(VideoFrame {
            pts: self.pts,
            duration: self.duration,
            width: self.width,
            height: self.height,
            format: PixelFormat::Nv12,
            storage: VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 {
                texture,
                subresource: self.subresource,
            }),
        })
    }
}

/// A decoded frame awaiting [`poll_frame`](VideoDecoder::poll_frame) — GPU texture hold
/// (Zero-Copy) or an already-owned CPU frame (software decode path).
enum PendingFrame {
    Gpu(GpuFrameHold),
    Cpu(VideoFrame),
}

/// H.264 decode session (DX11 Zero-Copy output, or CPU/software output).
pub(crate) struct WmfH264Decoder {
    transform: IMFTransform,
    info: StreamInfo,
    time_base_num: u64,
    time_base_den: u32,
    pending: VecDeque<PendingFrame>,
    /// COM hold for the GPU frame last returned from [`poll_frame`](VideoDecoder::poll_frame).
    released: Option<GpuFrameHold>,
    flushed: bool,
    dx11: Option<Dx11Session>,
    /// MFT-reported output buffer size; only used when the MFT does not provide its own
    /// samples (the CPU/software path).
    output_buf_size: u32,
    /// `Some(n)` when `extra_data`/packets are AVCC-framed (`n`-byte NAL length prefix,
    /// as produced by demuxed MP4 samples) and must be converted to Annex-B before
    /// reaching the MFT; `None` when already Annex-B (e.g. straight from an encoder).
    nal_length_size: Option<u8>,
}

impl WmfH264Decoder {
    /// Open according to [`VideoDecoderConfig::output`].
    pub(crate) fn open(config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        validate_common(config)?;
        ensure_mf_runtime()?;
        match config.output {
            VideoOutputPreference::ZeroCopyGpu => Self::open_dx11(config),
            VideoOutputPreference::CpuFramesOk => Self::open_cpu(config),
            _ => Err(DecodeError::Unsupported),
        }
    }

    fn open_dx11(config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        let Some(GpuDeviceHandle::DirectX11(handle)) = config.gpu_device else {
            return Err(DecodeError::InvalidInput);
        };
        let input_subtype = video_subtype(config.codec)?;
        let device = dx11::device_from_handle(handle)?;
        let (annex_b_extra_data, nal_length_size) = resolve_annex_b_extra_data(&config.extra_data);
        let (transform, session) = dx11::open_hw_decoder(
            device,
            config.width,
            config.height,
            &annex_b_extra_data,
            &input_subtype,
        )?;
        let output_buf_size = output_buffer_size(&transform)?;
        Ok(Self {
            transform,
            info: stream_info_from(config),
            time_base_num: config.time_base.num,
            time_base_den: config.time_base.den,
            pending: VecDeque::new(),
            released: None,
            flushed: false,
            dx11: Some(session),
            output_buf_size,
            nal_length_size,
        })
    }

    /// Open the software H.264 decoder MFT — no `ID3D11Device`, no DXGI device manager;
    /// frames come back as [`VideoFrameStorage::Cpu`] copied straight out of the MFT's
    /// system-memory output buffer (see [`super::cpu`] — this is honest CPU decode, not a
    /// GPU→CPU readback, since there is no GPU texture in this path).
    fn open_cpu(config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        let input_subtype = video_subtype(config.codec)?;
        let transform = open_sw_decoder(&input_subtype)?;
        let (annex_b_extra_data, nal_length_size) = resolve_annex_b_extra_data(&config.extra_data);
        configure_decode_types(
            &transform,
            config.width,
            config.height,
            &annex_b_extra_data,
            &input_subtype,
        )?;
        begin_streaming(&transform)?;
        let output_buf_size = output_buffer_size(&transform)?;
        Ok(Self {
            transform,
            info: stream_info_from(config),
            time_base_num: config.time_base.num,
            time_base_den: config.time_base.den,
            pending: VecDeque::new(),
            released: None,
            flushed: false,
            dx11: None,
            output_buf_size,
            nal_length_size,
        })
    }

    fn recycle_surfaces(&mut self) {
        self.released = None;
    }

    fn push_transform_packet(&mut self, packet: &Packet) -> Result<(), DecodeError> {
        if let Some(session) = self.dx11.as_mut() {
            dx11::drain_events_nonblocking(session)?;
            dx11::wait_need_input(session)?;
        }
        let sample = packet_to_sample(
            packet,
            self.time_base_num,
            self.time_base_den,
            self.nal_length_size,
        )?;
        unsafe { self.transform.ProcessInput(0, &sample, 0) }.map_err(|_| DecodeError::Backend)?;
        if let Some(session) = self.dx11.as_mut() {
            dx11::consume_need_input(session);
            dx11::drain_events_nonblocking(session)?;
        }
        self.drain_output()?;
        Ok(())
    }

    fn drain_output(&mut self) -> Result<(), DecodeError> {
        let provides = self
            .dx11
            .as_ref()
            .is_some_and(|s| s.output_provides_samples);
        loop {
            if let Some(session) = self.dx11.as_mut() {
                dx11::drain_events_nonblocking(session)?;
            }
            match process_one_output(&self.transform, provides, self.output_buf_size)? {
                Drain::Sample(sample) => {
                    self.adopt_output_sample(sample)?;
                }
                Drain::NeedMore => break,
                Drain::StreamChange => self.apply_stream_change()?,
            }
        }
        Ok(())
    }

    fn apply_stream_change(&mut self) -> Result<(), DecodeError> {
        let (width, height) = read_output_dimensions(&self.transform)?;
        if let StreamInfo::Video { geometry, .. } = &mut self.info {
            *geometry = VideoGeometry { width, height };
        }
        // Re-apply types after stream change (best effort).
        let input_subtype = video_subtype(self.info.codec())?;
        let (annex_b_extra_data, nal_length_size) =
            resolve_annex_b_extra_data(self.info.extra_data());
        configure_decode_types(
            &self.transform,
            width,
            height,
            &annex_b_extra_data,
            &input_subtype,
        )?;
        self.nal_length_size = nal_length_size;
        begin_streaming(&self.transform)?;
        self.output_buf_size = output_buffer_size(&self.transform)?;
        Ok(())
    }

    fn adopt_output_sample(&mut self, sample: IMFSample) -> Result<(), DecodeError> {
        let pts_hns = unsafe { sample.GetSampleTime() }.unwrap_or(0);
        let dur_hns = unsafe { sample.GetSampleDuration() }.unwrap_or(0);
        let pts = from_hns(pts_hns, self.time_base_num, self.time_base_den);
        let duration =
            u64::try_from(from_hns(dur_hns, self.time_base_num, self.time_base_den).max(0))
                .unwrap_or(0);
        let geometry = self.info.geometry().unwrap_or(VideoGeometry {
            width: 0,
            height: 0,
        });
        let width = geometry.width;
        let height = geometry.height;

        if self.dx11.is_some() {
            let (texture, subresource) = dx11::texture_from_output_sample(&sample)?;
            self.pending.push_back(PendingFrame::Gpu(GpuFrameHold {
                _sample: sample,
                texture,
                subresource,
                pts,
                duration,
                width,
                height,
            }));
        } else {
            let data = cpu::nv12_bytes_from_output_sample(&sample, width, height)?;
            self.pending.push_back(PendingFrame::Cpu(VideoFrame {
                pts,
                duration,
                width,
                height,
                format: PixelFormat::Nv12,
                storage: VideoFrameStorage::Cpu { data },
            }));
        }
        Ok(())
    }
}

impl VideoDecoder for WmfH264Decoder {
    fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    fn push_packet(&mut self, packet: &Packet) -> Result<(), DecodeError> {
        if self.flushed {
            return Err(DecodeError::Closed);
        }
        self.recycle_surfaces();
        if packet.is_discard {
            return Ok(());
        }
        self.push_transform_packet(packet)
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        if self.pending.is_empty() {
            self.drain_output()?;
        }
        let Some(pending) = self.pending.pop_front() else {
            return Ok(None);
        };
        self.recycle_surfaces();
        match pending {
            PendingFrame::Gpu(hold) => {
                let frame = hold.to_video_frame()?;
                self.released = Some(hold);
                Ok(Some(frame))
            }
            PendingFrame::Cpu(frame) => Ok(Some(frame)),
        }
    }

    fn flush(&mut self) -> Result<(), DecodeError> {
        if self.flushed {
            return Ok(());
        }
        self.recycle_surfaces();
        self.flushed = true;
        unsafe {
            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)
                .map_err(|_| DecodeError::Backend)?;
            self.transform
                .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)
                .map_err(|_| DecodeError::Backend)?;
        }
        self.drain_output()?;
        notify_end_streaming(&self.transform);
        Ok(())
    }
}

fn validate_common(config: &VideoDecoderConfig) -> Result<(), DecodeError> {
    if !is_supported_video_codec(config.codec) {
        return Err(DecodeError::Unsupported);
    }
    if config.pixel_format != PixelFormat::Nv12 {
        return Err(DecodeError::Unsupported);
    }
    if config.time_base.den == 0 {
        return Err(DecodeError::InvalidInput);
    }
    Ok(())
}

#[allow(clippy::missing_const_for_fn, reason = "StreamInfo holds Bytes")]
fn stream_info_from(config: &VideoDecoderConfig) -> StreamInfo {
    StreamInfo::Video {
        id: 0,
        codec: config.codec,
        time_base: config.time_base,
        geometry: VideoGeometry {
            width: config.width,
            height: config.height,
        },
        extra_data: config.extra_data.clone(), // clone: owned StreamInfo snapshot at open
    }
}

fn ensure_mf_runtime() -> Result<(), DecodeError> {
    super::runtime::ensure_mf()
}

/// Detect AVCC-framed `extra_data` (an `AVCDecoderConfigurationRecord`, as produced by
/// demuxed MP4 samples) and convert it to the Annex-B sequence header MF's
/// `MF_MT_MPEG_SEQUENCE_HEADER` attribute expects, returning the NAL length size so
/// per-packet payloads can be converted the same way. Passes Annex-B input through
/// unchanged (e.g. `extra_data` straight from an encoder's own stream info).
fn resolve_annex_b_extra_data(extra_data: &Bytes) -> (Bytes, Option<u8>) {
    match parse_avc_decoder_config(extra_data) {
        Some(config) => {
            let nal_length_size = config.nal_length_size;
            (annex_b_sequence_header(&config), Some(nal_length_size))
        }
        None => (extra_data.clone(), None), // clone: Bytes ref-count bump, not a payload copy
    }
}
