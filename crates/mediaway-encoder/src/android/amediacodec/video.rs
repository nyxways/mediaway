//! `AMediaCodec` H.264 CPU-upload encode session.
//!
//! See [ADR-0001](../../adr/android/0001-ndk-amediacodec-h264-cpu-upload.md): binding choice,
//! scope (H.264 / CPU YUV420 upload only / best-effort all-sync-frame), and the
//! zero-compile-verification caveat for this crate as authored.

use std::collections::VecDeque;
use std::time::Duration;

use crate::{EncodeError, VideoEncoder, VideoEncoderConfig, VideoInputPreference};
use mediaway_common::{
    Bytes, CodecKind, Packet, PixelFormat, StreamInfo, VideoFrame, VideoFrameStorage, VideoGeometry,
};

use ndk::media::media_codec::{
    DequeuedInputBufferResult, DequeuedOutputBufferInfoResult, MediaCodec, MediaCodecDirection,
};
use ndk::media::media_format::MediaFormat;

use super::codec::mime_type;

/// `MediaCodecInfo.CodecCapabilities.COLOR_FormatYUV420SemiPlanar` — the NV12-compatible
/// input layout requested via `KEY_COLOR_FORMAT`. Exact channel-order/stride quirks are
/// device-dependent (a well-known `AMediaCodec` gotcha); unverified against real hardware
/// this stage — see ADR-0001.
const COLOR_FORMAT_YUV420_SEMI_PLANAR: i32 = 21;

/// `android.media.MediaCodec.BUFFER_FLAG_END_OF_STREAM`.
const BUFFER_FLAG_END_OF_STREAM: u32 = 4;
/// `android.media.MediaCodec.BUFFER_FLAG_CODEC_CONFIG`.
const BUFFER_FLAG_CODEC_CONFIG: u32 = 2;
/// `android.media.MediaCodec.BUFFER_FLAG_KEY_FRAME` (same bit as the older
/// `BUFFER_FLAG_SYNC_FRAME`).
const BUFFER_FLAG_KEY_FRAME: u32 = 1;

/// Per-call `dequeue_input_buffer` timeout while retrying for a free input slot.
const INPUT_DEQUEUE_TIMEOUT: Duration = Duration::from_millis(20);
/// Max `dequeue_input_buffer` retries in [`AmediaCodecVideoEncoder::push_frame`] before giving
/// up (≈1s worst case) — `AMediaCodec` input starvation this long indicates a stuck session,
/// not normal backpressure.
const MAX_INPUT_RETRIES: usize = 50;
/// Per-call `dequeue_output_buffer` timeout for the opportunistic drain in `push_frame`/
/// `poll_packet` — short, since output may simply not be ready yet.
const OUTPUT_DEQUEUE_TIMEOUT: Duration = Duration::from_millis(5);
/// Max opportunistic output-drain attempts per `push_frame`/`poll_packet` call.
const MAX_OPPORTUNISTIC_OUTPUT_DRAIN: usize = 4;
/// Max `dequeue_output_buffer` retries in [`AmediaCodecVideoEncoder::flush`] while waiting for
/// the end-of-stream buffer (≈2s worst case).
const MAX_FLUSH_DRAIN_RETRIES: usize = 400;

/// `AMediaCodec` H.264 encode session (CPU YUV420-semi-planar upload, best-effort all-sync).
pub(crate) struct AmediaCodecVideoEncoder {
    codec: MediaCodec,
    info: StreamInfo,
    width: u32,
    height: u32,
    yuv420_bytes: usize,
    pending: VecDeque<Packet>,
    flushed: bool,
}

impl AmediaCodecVideoEncoder {
    /// Open according to [`VideoEncoderConfig::input`].
    pub(crate) fn open(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        validate(config)?;
        match config.input {
            VideoInputPreference::CpuUploadOk => Self::open_cpu(config),
            // `AHardwareBuffer`/`ANativeWindow` Zero-Copy input is deferred — see ADR-0001 §
            // Scope / roadmap.
            _ => Err(EncodeError::Unsupported),
        }
    }

    fn open_cpu(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        let mime = mime_type(config.codec)?;
        // A real, honest failure — not every AOSP device is guaranteed a given codec name,
        // though the Android CDD requires at least one AVC encoder. See ADR-0001.
        let codec = MediaCodec::from_encoder_type(mime).ok_or(EncodeError::Backend)?;

        let mut format = MediaFormat::new();
        format.set_str("mime", mime);
        format.set_i32(
            "width",
            i32::try_from(config.width).map_err(|_| EncodeError::InvalidInput)?,
        );
        format.set_i32(
            "height",
            i32::try_from(config.height).map_err(|_| EncodeError::InvalidInput)?,
        );
        format.set_i32("color-format", COLOR_FORMAT_YUV420_SEMI_PLANAR);
        format.set_i32(
            "bitrate",
            i32::try_from(config.bitrate_bps).unwrap_or(i32::MAX),
        );
        let frame_rate = frame_rate_hint(config.time_base);
        format.set_i32("frame-rate", frame_rate);
        format.set_f32(
            "i-frame-interval",
            i_frame_interval_secs(config.gop_size, frame_rate),
        );

        codec
            .configure(&format, None, MediaCodecDirection::Encoder)
            .map_err(|_| EncodeError::Backend)?;
        codec.start().map_err(|_| EncodeError::Backend)?;

        let yuv420_bytes = yuv420_size(config.width, config.height)?;

        Ok(Self {
            codec,
            info: stream_info_from(config),
            width: config.width,
            height: config.height,
            yuv420_bytes,
            pending: VecDeque::new(),
            flushed: false,
        })
    }

    /// Copy `data` into the next available input buffer and queue it, retrying up to
    /// [`MAX_INPUT_RETRIES`] times while the codec has none free.
    fn upload_and_queue(&self, data: &[u8], time_us: u64, flags: u32) -> Result<(), EncodeError> {
        for _ in 0..MAX_INPUT_RETRIES {
            match self
                .codec
                .dequeue_input_buffer(INPUT_DEQUEUE_TIMEOUT)
                .map_err(|_| EncodeError::Backend)?
            {
                DequeuedInputBufferResult::Buffer(mut input_buffer) => {
                    let dst = input_buffer.buffer_mut();
                    let n = data.len().min(dst.len());
                    for (d, &s) in dst[..n].iter_mut().zip(data[..n].iter()) {
                        d.write(s);
                    }
                    self.codec
                        .queue_input_buffer(input_buffer, 0, n, time_us, flags)
                        .map_err(|_| EncodeError::Backend)?;
                    return Ok(());
                }
                DequeuedInputBufferResult::TryAgainLater => continue,
            }
        }
        Err(EncodeError::Backend)
    }

    /// Opportunistically drain whatever output is already ready into `self.pending`, up to
    /// `max_attempts` short-timeout polls. Does not block waiting for more input to be
    /// consumed — `AMediaCodec` output readiness relative to a given `push_frame` call is not
    /// guaranteed synchronous (unlike the Linux VA-API backend's `vaSyncSurface`-backed
    /// per-frame completion) — see ADR-0001.
    fn drain_output(&mut self, max_attempts: usize) -> Result<(), EncodeError> {
        for _ in 0..max_attempts {
            match self
                .codec
                .dequeue_output_buffer(OUTPUT_DEQUEUE_TIMEOUT)
                .map_err(|_| EncodeError::Backend)?
            {
                DequeuedOutputBufferInfoResult::Buffer(output_buffer) => {
                    let info = *output_buffer.info();
                    let flags = info.flags();
                    if flags & BUFFER_FLAG_CODEC_CONFIG != 0 {
                        // SPS/PPS codec-config buffer, not frame data — extradata capture
                        // (`csd-0`/`csd-1`) is deferred this stage, see ADR-0001 § Scope.
                        self.codec
                            .release_output_buffer(output_buffer, false)
                            .map_err(|_| EncodeError::Backend)?;
                        continue;
                    }
                    let start = usize::try_from(info.offset()).unwrap_or(0);
                    let len = usize::try_from(info.size()).unwrap_or(0);
                    let full = output_buffer.buffer();
                    let end = start.saturating_add(len).min(full.len());
                    let payload = full.get(start..end).unwrap_or(&[]).to_vec();
                    let pts = i64::try_from(info.presentation_time_us()).unwrap_or(0);
                    let is_eos = flags & BUFFER_FLAG_END_OF_STREAM != 0;
                    self.codec
                        .release_output_buffer(output_buffer, false)
                        .map_err(|_| EncodeError::Backend)?;
                    if !payload.is_empty() {
                        self.pending.push_back(Packet {
                            stream_id: 0,
                            pts,
                            dts: pts,
                            duration: 0,
                            is_keyframe: flags & BUFFER_FLAG_KEY_FRAME != 0,
                            is_discard: false,
                            payload: Bytes::from(payload),
                        });
                    }
                    if is_eos {
                        return Ok(());
                    }
                }
                DequeuedOutputBufferInfoResult::TryAgainLater => return Ok(()),
                // Format/buffer-set change events carry no packet payload this stage — skip
                // and keep draining (extradata capture from the changed format is deferred,
                // same as the codec-config-buffer branch above).
                DequeuedOutputBufferInfoResult::OutputFormatChanged
                | DequeuedOutputBufferInfoResult::OutputBuffersChanged => {}
            }
        }
        Ok(())
    }
}

impl VideoEncoder for AmediaCodecVideoEncoder {
    fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    fn push_frame(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        if self.flushed {
            return Err(EncodeError::Closed);
        }
        let VideoFrameStorage::Cpu { data } = &frame.storage else {
            return Err(EncodeError::Unsupported);
        };
        if frame.width != self.width || frame.height != self.height {
            return Err(EncodeError::InvalidInput);
        }
        if data.len() < self.yuv420_bytes {
            return Err(EncodeError::InvalidInput);
        }

        let time_us = u64::try_from(frame.pts).unwrap_or(0);
        self.upload_and_queue(&data[..self.yuv420_bytes], time_us, 0)?;
        self.drain_output(MAX_OPPORTUNISTIC_OUTPUT_DRAIN)
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        if self.pending.is_empty() {
            self.drain_output(1)?;
        }
        Ok(self.pending.pop_front())
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        if self.flushed {
            return Ok(());
        }
        self.upload_and_queue(&[], 0, BUFFER_FLAG_END_OF_STREAM)?;
        self.drain_output(MAX_FLUSH_DRAIN_RETRIES)?;
        self.flushed = true;
        Ok(())
    }
}

/// `KEY_I_FRAME_INTERVAL` is seconds between key frames, not a frame count — `0` requests every
/// frame as a sync frame on most devices (the `gop_size <= 1`, IDR-only case), otherwise convert
/// `gop_size` (frames) through `frame_rate` into seconds. Device-dependent, not a hard spec
/// guarantee — the closest lever to Linux ADR-0001's deterministic per-frame-IDR guarantee. See
/// this module's doc comment.
fn i_frame_interval_secs(gop_size: u32, frame_rate: i32) -> f32 {
    if gop_size <= 1 {
        0.0
    } else {
        gop_size as f32 / frame_rate.max(1) as f32
    }
}

/// Nearest-integer frames-per-second hint for `KEY_FRAME_RATE` from a timebase — `AMediaCodec`
/// wants a plain rate, not a rational; this is a lossy hint only (bitrate pacing), not used
/// for output packet timing (packets carry `presentation_time_us` from the codec directly).
fn frame_rate_hint(time_base: mediaway_common::Rational) -> i32 {
    if time_base.num == 0 {
        return 30;
    }
    i32::try_from(time_base.den / time_base.num)
        .unwrap_or(30)
        .max(1)
}

fn validate(config: &VideoEncoderConfig) -> Result<(), EncodeError> {
    if !super::codec::is_supported_video_codec(config.codec) {
        return Err(EncodeError::Unsupported);
    }
    if config.width == 0 || config.height == 0 {
        return Err(EncodeError::InvalidInput);
    }
    if config.pixel_format != PixelFormat::Nv12 {
        return Err(EncodeError::Unsupported);
    }
    if config.time_base.den == 0 {
        return Err(EncodeError::InvalidInput);
    }
    Ok(())
}

fn yuv420_size(width: u32, height: u32) -> Result<usize, EncodeError> {
    let w = usize::try_from(width).map_err(|_| EncodeError::InvalidInput)?;
    let h = usize::try_from(height).map_err(|_| EncodeError::InvalidInput)?;
    w.checked_mul(h)
        .and_then(|y| y.checked_mul(3))
        .and_then(|v| v.checked_div(2))
        .ok_or(EncodeError::InvalidInput)
}

#[allow(clippy::missing_const_for_fn, reason = "StreamInfo holds Bytes")]
fn stream_info_from(config: &VideoEncoderConfig) -> StreamInfo {
    StreamInfo::Video {
        id: 0,
        codec: CodecKind::H264,
        time_base: config.time_base,
        geometry: VideoGeometry {
            width: config.width,
            height: config.height,
        },
        extra_data: Bytes::new(),
    }
}

#[cfg(test)]
#[path = "video_tests.rs"]
mod tests;
