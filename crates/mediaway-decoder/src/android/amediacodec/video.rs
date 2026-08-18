//! `AMediaCodec` H.264 CPU-output decode session.
//!
//! See [ADR android/0001](../../adr/android/0001-ndk-amediacodec-h264-cpu-out.md): binding
//! choice, scope (H.264 / CPU NV12 output only / `COLOR_FormatYUV420SemiPlanar` only / general
//! GOP, not IDR-only), and the zero-compile/zero-runtime-verification caveat for this crate as
//! authored.

use std::collections::VecDeque;
use std::time::Duration;

use crate::{DecodeError, VideoDecoder, VideoDecoderConfig, VideoOutputPreference};
use mediaway_common::{
    CodecKind, Packet, PixelFormat, Rational, StreamInfo, VideoFrame, VideoFrameStorage,
    VideoGeometry,
};

use ndk::media::media_codec::{
    BufferInfo, DequeuedInputBufferResult, DequeuedOutputBufferInfoResult, MediaCodec,
    MediaCodecDirection,
};
use ndk::media::media_format::MediaFormat;

use super::codec::{is_supported_video_codec, mime_type};
use super::csd::split_csd;
use super::nv12::{CropRect, strip_and_crop_nv12};

/// `MediaCodecInfo.CodecCapabilities.COLOR_FormatYUV420SemiPlanar` — the only output color
/// format this backend accepts (reject-not-guess policy, see ADR android/0001 § Decision).
const COLOR_FORMAT_YUV420_SEMI_PLANAR: i32 = 21;

/// `android.media.MediaCodec.BUFFER_FLAG_END_OF_STREAM`.
const BUFFER_FLAG_END_OF_STREAM: u32 = 4;

/// Per-call `dequeue_input_buffer` timeout while retrying for a free input slot.
const INPUT_DEQUEUE_TIMEOUT: Duration = Duration::from_millis(20);
/// Max `dequeue_input_buffer` retries in [`AmediaCodecVideoDecoder::upload_and_queue`] before
/// giving up (≈1s worst case) — `AMediaCodec` input starvation this long indicates a stuck
/// session, not normal backpressure.
const MAX_INPUT_RETRIES: usize = 50;
/// Per-call `dequeue_output_buffer` timeout for the opportunistic drain in `push_packet`/
/// `poll_frame` — short, since output may simply not be ready yet.
const OUTPUT_DEQUEUE_TIMEOUT: Duration = Duration::from_millis(5);
/// Max opportunistic output-drain attempts per `push_packet`/`poll_frame` call.
const MAX_OPPORTUNISTIC_OUTPUT_DRAIN: usize = 4;
/// Max `dequeue_output_buffer` retries in [`AmediaCodecVideoDecoder::flush`] while waiting for
/// the end-of-stream buffer (≈2s worst case).
const MAX_FLUSH_DRAIN_RETRIES: usize = 400;

/// Output ByteBuffer layout, cached from `output_format()` on the first `OutputFormatChanged`
/// event. `crop` is `(left, top, right, bottom)`, per ADR android/0001's ZCA shape.
#[derive(Debug, Clone, Copy)]
struct OutputLayout {
    stride: u32,
    slice_height: u32,
    crop: (u32, u32, u32, u32),
}

/// `AMediaCodec` H.264 decode session (CPU NV12 output, general GOP — no DPB in this crate,
/// the device manages reference frames internally).
pub(crate) struct AmediaCodecVideoDecoder {
    codec: MediaCodec,
    info: StreamInfo,
    time_base: Rational,
    /// Set on the first `OutputFormatChanged` event; `None` before that.
    output_layout: Option<OutputLayout>,
    pending: VecDeque<VideoFrame>,
    flushed: bool,
}

impl AmediaCodecVideoDecoder {
    /// Open according to [`VideoDecoderConfig`].
    pub(crate) fn open(config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        validate(config)?;
        if config.output != VideoOutputPreference::CpuFramesOk {
            // Zero-Copy `Surface` output deferred — see ADR android/0001 § Scope.
            return Err(DecodeError::Unsupported);
        }

        let mime = mime_type(config.codec)?;
        // A real, honest failure — the Android CDD requires at least one AVC decoder, but this
        // crate does not assume a specific one exists. See ADR android/0001 § Decision.
        let codec = MediaCodec::from_decoder_type(mime).ok_or(DecodeError::Backend)?;

        let mut format = MediaFormat::new();
        format.set_str("mime", mime);
        format.set_i32(
            "width",
            i32::try_from(config.width).map_err(|_| DecodeError::InvalidInput)?,
        );
        format.set_i32(
            "height",
            i32::try_from(config.height).map_err(|_| DecodeError::InvalidInput)?,
        );

        // CSD handoff is best-effort, not required — see `super::csd` module docs.
        if !config.extra_data.is_empty() {
            let (sps, pps) = split_csd(config.extra_data.as_ref());
            if let Some(sps) = sps.as_deref() {
                format.set_buffer("csd-0", sps);
            }
            if let Some(pps) = pps.as_deref() {
                format.set_buffer("csd-1", pps);
            }
        }

        codec
            .configure(&format, None, MediaCodecDirection::Decoder)
            .map_err(|_| DecodeError::Backend)?;
        codec.start().map_err(|_| DecodeError::Backend)?;

        Ok(Self {
            codec,
            info: stream_info_from(config),
            time_base: config.time_base,
            output_layout: None,
            pending: VecDeque::new(),
            flushed: false,
        })
    }

    /// Copy `data` into the next available input buffer and queue it, retrying up to
    /// [`MAX_INPUT_RETRIES`] times while the codec has none free. `time_us` is
    /// [`time_us_from`]'s output — `AMediaCodec`'s own time domain, not `time_base` units.
    fn upload_and_queue(&self, data: &[u8], time_us: u64, flags: u32) -> Result<(), DecodeError> {
        for _ in 0..MAX_INPUT_RETRIES {
            match self
                .codec
                .dequeue_input_buffer(INPUT_DEQUEUE_TIMEOUT)
                .map_err(|_| DecodeError::Backend)?
            {
                DequeuedInputBufferResult::Buffer(mut input_buffer) => {
                    let dst = input_buffer.buffer_mut();
                    let n = data.len().min(dst.len());
                    for (d, &s) in dst[..n].iter_mut().zip(data[..n].iter()) {
                        d.write(s);
                    }
                    self.codec
                        .queue_input_buffer(input_buffer, 0, n, time_us, flags)
                        .map_err(|_| DecodeError::Backend)?;
                    return Ok(());
                }
                DequeuedInputBufferResult::TryAgainLater => {}
            }
        }
        Err(DecodeError::Backend)
    }

    /// Opportunistically drain whatever output is already ready into `self.pending`, up to
    /// `max_attempts` short-timeout polls. Does not block waiting for more input — `AMediaCodec`
    /// output readiness relative to a given `push_packet` call is not guaranteed synchronous
    /// (mirrors `mediaway-encoder-android`'s identical `drain_output` shape).
    fn drain_output(&mut self, max_attempts: usize) -> Result<(), DecodeError> {
        for _ in 0..max_attempts {
            match self
                .codec
                .dequeue_output_buffer(OUTPUT_DEQUEUE_TIMEOUT)
                .map_err(|_| DecodeError::Backend)?
            {
                DequeuedOutputBufferInfoResult::Buffer(output_buffer) => {
                    let info = *output_buffer.info();
                    let is_eos = info.flags() & BUFFER_FLAG_END_OF_STREAM != 0;
                    let full = output_buffer.buffer();
                    let frame = self
                        .output_layout
                        .map(|layout| build_frame(full, layout, &info, self.time_base));
                    self.codec
                        .release_output_buffer(output_buffer, false)
                        .map_err(|_| DecodeError::Backend)?;
                    if let Some(frame) = frame {
                        self.pending.push_back(frame);
                    }
                    if is_eos {
                        return Ok(());
                    }
                }
                DequeuedOutputBufferInfoResult::OutputFormatChanged => {
                    self.adopt_output_format()?;
                }
                DequeuedOutputBufferInfoResult::TryAgainLater => return Ok(()),
                // No buffer-set-change concept for this crate's flat index-based drain —
                // `AMediaCodec_getOutputBuffer` is re-resolved per index on every dequeue.
                DequeuedOutputBufferInfoResult::OutputBuffersChanged => {}
            }
        }
        Ok(())
    }

    /// Read the negotiated output format after an `OutputFormatChanged` event: validate
    /// `"color-format"` (reject, never guess — see ADR android/0001 § Decision), cache
    /// `stride`/`slice-height`/crop as this session's [`OutputLayout`], and update
    /// `stream_info()`'s `VideoGeometry` from the crop rect.
    fn adopt_output_format(&mut self) -> Result<(), DecodeError> {
        let format = self.codec.output_format();
        let color_format = format.i32("color-format").ok_or(DecodeError::Backend)?;
        if color_format != COLOR_FORMAT_YUV420_SEMI_PLANAR {
            // `COLOR_FormatYUV420Planar`, `COLOR_FormatYUV420Flexible`, or a vendor-specific
            // constant — out of scope this stage, see ADR android/0001 § Scope.
            return Err(DecodeError::Unsupported);
        }

        let coded_width = format.i32("width").ok_or(DecodeError::Backend)?;
        let coded_height = format.i32("height").ok_or(DecodeError::Backend)?;

        let stride = format
            .i32("stride")
            .and_then(|v| u32::try_from(v).ok())
            .ok_or(DecodeError::Backend)?;
        // Missing/zero `slice-height` is a documented "same as height" quirk on some devices —
        // see ADR android/0001 § Decision.
        let slice_height = format
            .i32("slice-height")
            .filter(|&v| v > 0)
            .and_then(|v| u32::try_from(v).ok())
            .or_else(|| u32::try_from(coded_height).ok())
            .ok_or(DecodeError::Backend)?;

        let crop = (
            crop_component(&format, "crop-left", 0),
            crop_component(&format, "crop-top", 0),
            crop_component(&format, "crop-right", coded_width),
            crop_component(&format, "crop-bottom", coded_height),
        );

        self.output_layout = Some(OutputLayout {
            stride,
            slice_height,
            crop,
        });

        if let StreamInfo::Video { geometry, .. } = &mut self.info {
            *geometry = VideoGeometry {
                width: crop.2.saturating_sub(crop.0),
                height: crop.3.saturating_sub(crop.1),
            };
        }
        Ok(())
    }
}

/// Read `key` as a non-negative `u32`, falling back to `fallback` when the key is absent (not
/// every device reports all four crop keys) or negative (malformed).
fn crop_component(format: &MediaFormat, key: &str, fallback: i32) -> u32 {
    let raw = format.i32(key).unwrap_or(fallback);
    u32::try_from(raw).unwrap_or(0)
}

/// Strip `payload`'s `BufferInfo::offset()`..`+size()` valid region into a cropped, tightly
/// packed NV12 [`VideoFrame`] using `layout` (`super::nv12::strip_and_crop_nv12`).
fn build_frame(
    payload: &[u8],
    layout: OutputLayout,
    info: &BufferInfo,
    time_base: Rational,
) -> VideoFrame {
    let start = usize::try_from(info.offset()).unwrap_or(0);
    let len = usize::try_from(info.size()).unwrap_or(0);
    let end = start.saturating_add(len).min(payload.len());
    let region = payload.get(start..end).unwrap_or(&[]);

    let crop = CropRect {
        left: layout.crop.0,
        top: layout.crop.1,
        right: layout.crop.2,
        bottom: layout.crop.3,
    };
    let data = strip_and_crop_nv12(region, layout.stride, layout.slice_height, crop);

    VideoFrame {
        pts: pts_from_time_us(info.presentation_time_us(), time_base),
        // `AMediaCodec` output buffers carry no duration — unknown, per
        // `VideoFrame::duration`'s own doc comment ("`0` if unknown").
        duration: 0,
        width: crop.width(),
        height: crop.height(),
        format: PixelFormat::Nv12,
        storage: VideoFrameStorage::Cpu { data },
    }
}

/// Convert a packet timestamp (`time_base` units) to `AMediaCodec`'s microsecond time domain
/// (`queue_input_buffer`'s `time` parameter) — named per ADR android/0001 § Decision. Uses
/// `i128` intermediates to avoid overflow across large `pts`/timebase combinations.
fn time_us_from(pts: i64, time_base: Rational) -> u64 {
    if time_base.den == 0 {
        return 0;
    }
    let value = i128::from(pts) * i128::from(time_base.num) * 1_000_000 / i128::from(time_base.den);
    u64::try_from(value).unwrap_or(0)
}

/// Inverse of [`time_us_from`]: convert `AMediaCodec`'s `presentation_time_us` back into
/// `time_base` units for [`VideoFrame::pts`].
fn pts_from_time_us(time_us: i64, time_base: Rational) -> i64 {
    if time_base.num == 0 {
        return 0;
    }
    let denom = i128::from(time_base.num) * 1_000_000;
    if denom == 0 {
        return 0;
    }
    let value = i128::from(time_us) * i128::from(time_base.den) / denom;
    i64::try_from(value).unwrap_or(0)
}

impl VideoDecoder for AmediaCodecVideoDecoder {
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
        let time_us = time_us_from(packet.pts, self.time_base);
        self.upload_and_queue(packet.payload.as_ref(), time_us, 0)?;
        self.drain_output(MAX_OPPORTUNISTIC_OUTPUT_DRAIN)
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        if self.pending.is_empty() {
            self.drain_output(1)?;
        }
        Ok(self.pending.pop_front())
    }

    fn flush(&mut self) -> Result<(), DecodeError> {
        if self.flushed {
            return Ok(());
        }
        self.upload_and_queue(&[], 0, BUFFER_FLAG_END_OF_STREAM)?;
        self.drain_output(MAX_FLUSH_DRAIN_RETRIES)?;
        self.flushed = true;
        Ok(())
    }
}

fn validate(config: &VideoDecoderConfig) -> Result<(), DecodeError> {
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
        codec: CodecKind::H264,
        time_base: config.time_base,
        geometry: VideoGeometry {
            width: config.width,
            height: config.height,
        },
        extra_data: config.extra_data.clone(), // clone: owned StreamInfo snapshot at open
    }
}
