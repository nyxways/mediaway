//! CPU (software) decode for HEVC / AV1 / VP9 via enumerated Media Foundation decoder MFTs.
//!
//! Structurally this mirrors [`super::h264::WmfH264Decoder`]'s CPU-only path (open a
//! synchronous decoder MFT found through `MFTEnumEx`, configure an NV12 output type, drive
//! `ProcessInput`/`ProcessOutput` directly, copy planes out of the system-memory output
//! buffer) but drops H.264's DX11 Zero-Copy branch and its AVCC→Annex-B `extra_data`/NAL
//! conversion — HEVC/AV1/VP9 packets/`extra_data` here are used as-is (the packaging a
//! straight-from-`mediaway-encoder-windows` bitstream already has, same as H.264's
//! straight-from-encoder case in `resolve_annex_b_extra_data`).
//!
//! Real `MFTEnumEx(MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG_SYNCMFT | SORTANDFILTER, …)`
//! results per codec on the Windows 11 host this was verified on (RTX 4090 + Intel UHD 770)
//! are recorded in `docs/roadmap.md`, alongside the friendly-name enumeration in
//! `video_cpu_tests.rs::list_decoder_mfts_for_each_codec`.
//!
//! Wired into [`super::super::WindowsVideoDecoder`]'s dispatch for HEVC/AV1/VP9 codecs
//! (CPU output only; no DX11 Zero-Copy path exists for these codecs, unlike H.264).
//! This module provides a self-contained, real, and MFT-tested CPU decode path.

#![allow(unsafe_code)]

use std::collections::VecDeque;

use crate::{DecodeError, VideoDecoder, VideoDecoderConfig, VideoOutputPreference};
use mediaway_common::{
    CodecKind, Packet, PixelFormat, StreamInfo, VideoFrame, VideoFrameStorage, VideoGeometry,
};
use windows::Win32::Media::MediaFoundation::{
    IMFSample, IMFTransform, MF_E_NO_MORE_TYPES, MF_MT_SUBTYPE, MFT_MESSAGE_COMMAND_DRAIN,
    MFT_MESSAGE_NOTIFY_END_OF_STREAM, MFVideoFormat_NV12,
};

use super::codec::{is_supported_video_codec, video_subtype};
use super::cpu::{nv12_bytes_from_output_sample, open_sw_decoder};
use super::shared::{
    Drain, begin_streaming, configure_decode_types, notify_end_streaming, output_buffer_size,
    packet_to_sample, process_one_output, read_output_dimensions,
};

/// After `MF_E_TRANSFORM_STREAM_CHANGE`, adopt the decoder's own proposed NV12 output type
/// (`GetOutputAvailableType`/`SetOutputType`) instead of reconstructing one from a
/// previously-known width/height — see [`WmfMultiCodecCpuDecoder::apply_stream_change`] for
/// why that distinction matters here.
///
/// Real finding (this session, verified with a real system-`ffmpeg`/`libaom-av1`-encoded AV1
/// stream): the AV1 Store-extension decoder MFT on this machine only ever proposed
/// `MFVideoFormat_AYUV` for that content (`GetOutputAvailableType(0, 0)`), never NV12 — index
/// 1 immediately returns `MF_E_NO_MORE_TYPES`. So `DecodeError::Unsupported` (not `Backend`)
/// is returned when enumeration exhausts without an NV12 candidate: this crate's decode
/// sessions are NV12-only by design (`validate`), so a real MFT that only offers a different
/// subtype for a given real stream is an honest "unsupported for this pixel format", not a
/// transport failure.
fn negotiate_nv12_output_type(transform: &IMFTransform) -> Result<(), DecodeError> {
    for i in 0.. {
        let media_type = match unsafe { transform.GetOutputAvailableType(0, i) } {
            Ok(mt) => mt,
            Err(e) if e.code() == MF_E_NO_MORE_TYPES => return Err(DecodeError::Unsupported),
            Err(_) => return Err(DecodeError::Backend),
        };
        let subtype = unsafe { media_type.GetGUID(&MF_MT_SUBTYPE) }.unwrap_or_default();
        if subtype == MFVideoFormat_NV12 {
            unsafe { transform.SetOutputType(0, &media_type, 0) }
                .map_err(|_| DecodeError::Backend)?;
            return Ok(());
        }
    }
    Err(DecodeError::Unsupported)
}

/// CPU decode session for HEVC / AV1 / VP9 (no DX11, no `ID3D11Device`, no DXGI device
/// manager) — frames come back as [`VideoFrameStorage::Cpu`] copied straight out of the
/// MFT's system-memory output buffer, same honesty contract as
/// [`super::h264::WmfH264Decoder`]'s CPU path.
pub(crate) struct WmfMultiCodecCpuDecoder {
    transform: IMFTransform,
    info: StreamInfo,
    time_base_num: u64,
    time_base_den: u32,
    pending: VecDeque<VideoFrame>,
    flushed: bool,
    /// MFT-reported output buffer size — this path never gets MFT-provided samples
    /// (`MFT_ENUM_FLAG_SYNCMFT` decoders in practice do not set
    /// `MFT_OUTPUT_STREAM_PROVIDES_SAMPLES`).
    output_buf_size: u32,
}

impl WmfMultiCodecCpuDecoder {
    /// Open a software HEVC/AV1/VP9 decoder MFT for `config`.
    ///
    /// # Errors
    ///
    /// [`DecodeError::Unsupported`] when `config.codec` is not HEVC/AV1/VP9, the output
    /// pixel format is not NV12, or no matching decoder MFT is registered on this machine
    /// (see module docs for real per-codec `MFTEnumEx` findings). [`DecodeError::Backend`]
    /// on MF failure.
    pub(crate) fn open(config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        validate(config)?;
        super::runtime::ensure_mf()?;
        if config.output != VideoOutputPreference::CpuFramesOk {
            return Err(DecodeError::Unsupported);
        }
        let input_subtype = video_subtype(config.codec)?;
        let transform = open_sw_decoder(&input_subtype)?;
        configure_decode_types(
            &transform,
            config.width,
            config.height,
            &config.extra_data,
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
            flushed: false,
            output_buf_size,
        })
    }

    fn push_transform_packet(&mut self, packet: &Packet) -> Result<(), DecodeError> {
        let sample = packet_to_sample(packet, self.time_base_num, self.time_base_den, None)?;
        unsafe { self.transform.ProcessInput(0, &sample, 0) }.map_err(|_| DecodeError::Backend)?;
        self.drain_output()?;
        Ok(())
    }

    fn drain_output(&mut self) -> Result<(), DecodeError> {
        loop {
            match process_one_output(&self.transform, false, self.output_buf_size)? {
                Drain::Sample(sample) => self.adopt_output_sample(&sample)?,
                Drain::NeedMore => break,
                Drain::StreamChange => self.apply_stream_change()?,
            }
        }
        Ok(())
    }

    /// Handle `MF_E_TRANSFORM_STREAM_CHANGE` from `ProcessOutput`.
    ///
    /// Real finding (this session, verified with a live HEVC decode on this machine): unlike
    /// H.264's inbox decoder — where the width/height given at `open()` already matches what
    /// the bitstream's own SPS says, so [`super::h264::WmfH264Decoder`]'s
    /// `configure_decode_types`-based re-apply (rebuilding an output type from our own
    /// guessed `width`/`height`) never actually gets exercised — the HEVC/AV1 Store-extension
    /// decoder MFTs on this box only learn the real output geometry once they parse the
    /// first frame, and reject a caller-constructed output media type after the stream
    /// change (confirmed via a raw-HRESULT diagnostic: `SetOutputType` with our own type
    /// failed, while `SetOutputType` with the MFT's own `GetOutputAvailableType(0, i)`
    /// result succeeded and `ProcessOutput` then returned a real sample). So this negotiates
    /// the *MFT-proposed* NV12 output type instead of reconstructing one; the input type is
    /// untouched (still valid — only the output stream renegotiates).
    fn apply_stream_change(&mut self) -> Result<(), DecodeError> {
        negotiate_nv12_output_type(&self.transform)?;
        let (width, height) = read_output_dimensions(&self.transform)?;
        if let StreamInfo::Video { geometry, .. } = &mut self.info {
            *geometry = VideoGeometry { width, height };
        }
        self.output_buf_size = output_buffer_size(&self.transform)?;
        Ok(())
    }

    fn adopt_output_sample(&mut self, sample: &IMFSample) -> Result<(), DecodeError> {
        let pts_hns = unsafe { sample.GetSampleTime() }.unwrap_or(0);
        let dur_hns = unsafe { sample.GetSampleDuration() }.unwrap_or(0);
        let pts = super::runtime::from_hns(pts_hns, self.time_base_num, self.time_base_den);
        let duration = u64::try_from(
            super::runtime::from_hns(dur_hns, self.time_base_num, self.time_base_den).max(0),
        )
        .unwrap_or(0);
        let geometry = self.info.geometry().unwrap_or(VideoGeometry {
            width: 0,
            height: 0,
        });
        let width = geometry.width;
        let height = geometry.height;
        let data = nv12_bytes_from_output_sample(sample, width, height)?;
        self.pending.push_back(VideoFrame {
            pts,
            duration,
            width,
            height,
            format: PixelFormat::Nv12,
            storage: VideoFrameStorage::Cpu { data },
        });
        Ok(())
    }
}

impl VideoDecoder for WmfMultiCodecCpuDecoder {
    fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    fn push_packet(&mut self, packet: &Packet) -> Result<(), DecodeError> {
        if self.flushed {
            return Err(DecodeError::Closed);
        }
        if packet.is_discard {
            return Ok(());
        }
        self.push_transform_packet(packet)
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        if self.pending.is_empty() {
            self.drain_output()?;
        }
        Ok(self.pending.pop_front())
    }

    fn flush(&mut self) -> Result<(), DecodeError> {
        if self.flushed {
            return Ok(());
        }
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

fn validate(config: &VideoDecoderConfig) -> Result<(), DecodeError> {
    if !is_supported_video_codec(config.codec) || config.codec == CodecKind::H264 {
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

#[cfg(test)]
#[path = "video_cpu_tests.rs"]
mod tests;
