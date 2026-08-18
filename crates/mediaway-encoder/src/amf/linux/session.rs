//! AMD AMF H.264 CPU-upload encode session (`shiguredo_amf`).
//!
//! See [ADR-0002](../../adr/amf/0002-amf-linux-shiguredo-amf-h264-cpu-upload.md): binding
//! choice, the callback→poll bridge design, scope (H.264 / CPU upload only), and the
//! zero-hardware-verification caveat for this backend as authored.
//!
//! GOP: `gop_size` is passed straight through to `EncoderConfig::gop_pic_size` —
//! `shiguredo_amf` exposes it as a plain config field with no manual reference-list
//! bookkeeping needed on this crate's side (unlike `linux::vaapi`, which forces all-IDR this
//! stage). `intra_refresh_period` has no confirmed `shiguredo_amf` equivalent (`EncodeOptions`
//! only exposes a `frame_type` force-flag, `ReconfigureParams` none) — this backend cannot
//! honor it and silently falls back to `gop_size` behavior, per
//! `VideoEncoderConfig::intra_refresh_period`'s documented fallback contract.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::{EncodeError, VideoEncoder, VideoEncoderConfig, VideoInputPreference};
use mediaway_common::{
    Bytes, CodecKind, Packet, PixelFormat, StreamInfo, VideoFrame, VideoFrameStorage, VideoGeometry,
};

use shiguredo_amf::amf::{Plane, Surface};
use shiguredo_amf::ffi::AMF_PLANE_TYPE;
use shiguredo_amf::{
    CodecConfig, EncodeHandler, EncodeOptions, EncodedFrame, Encoder, EncoderConfig, FrameFormat,
    H264EncoderConfig, PictureType, RateControlMode, ReconfigureParams,
};

use super::codec;

/// Per-frame correlation data carried through [`Encoder::encode`]'s `user_data` parameter
/// and read back via `EncodedFrame::user_data` in [`packet_from_encoded_frame`].
/// `shiguredo_amf::EncodedFrame` does not expose a timestamp of its own — only
/// `buffer()`/`picture_type()`/`user_data()`/`into_parts()` (ADR-0002 § Research) — so this
/// is the sole source of truth this session uses for [`Packet::pts`]/[`Packet::duration`],
/// not `Surface::set_pts`/`set_duration` (still called before `encode` — AMD AMF's
/// documented per-surface timing contract some internal rate-control paths read from — but
/// this crate never relies on it being echoed back through `Buffer::get_pts()`).
#[derive(Debug, Clone, Copy)]
struct FrameMeta {
    pts: i64,
    duration: u64,
}

/// Bridges `shiguredo_amf`'s callback-driven [`EncodeHandler`] to this crate's poll-based
/// [`VideoEncoder`] trait (`push_frame` / `poll_packet` / `flush`) — see ADR-0002 §
/// "Callback -> poll bridge". `EncodeHandler` requires `Send + 'static` (confirmed via
/// docs.rs's auto-trait section for the trait this session), so the shared queue is
/// `Arc<Mutex<_>>` — the `Send`-required substitution ADR-0002's `Rc<RefCell<_>>` sketch
/// already anticipated, not an architectural surprise.
#[derive(Clone)]
struct PacketSink {
    // clone: Arc share — the same queue is observed by both the `EncodeHandler` callback
    // (moved into `Encoder::new`, see `AmfSession::open_cpu`) and `AmfSession::poll_packet`;
    // there is exactly one mutation point (the `Mutex`), no self-referential struct.
    queue: Arc<Mutex<VecDeque<Result<Packet, EncodeError>>>>,
}

impl EncodeHandler for PacketSink {
    type UserData = FrameMeta;
    type Error = EncodeError;

    fn on_encoded(&mut self, result: Result<EncodedFrame<Self::UserData>, Self::Error>) {
        let outcome = match result {
            Ok(frame) => Ok(packet_from_encoded_frame(&frame)),
            Err(e) => Err(e),
        };
        // A poisoned mutex (a prior lock holder panicked) silently drops this packet rather
        // than panicking here too — `on_encoded` runs on `shiguredo_amf`'s own internal
        // poll/worker thread (confirmed against source: `Encoder` owns a `poll_thread:
        // Option<JoinHandle<()>>`), genuinely concurrent with this session's owning thread,
        // not merely `Send`-bound by trait signature — the `Arc<Mutex<_>>` queue below is
        // load-bearing, not defensive.
        if let Ok(mut queue) = self.queue.lock() {
            queue.push_back(outcome);
        }
    }
}

impl From<shiguredo_amf::Error> for EncodeError {
    fn from(_: shiguredo_amf::Error) -> Self {
        // `shiguredo_amf::Error` is a struct, not an enum — only `status() ->
        // Option<AMF_RESULT>` (ADR-0002 § Research) — so there is no variant-level match to
        // build a richer mapping from. Every AMF failure collapses to `Backend`, the same
        // convention `linux::vaapi` uses for its own opaque `VaError`.
        Self::Backend
    }
}

fn packet_from_encoded_frame(frame: &EncodedFrame<FrameMeta>) -> Packet {
    let meta = *frame.user_data();
    let is_keyframe = matches!(frame.picture_type(), PictureType::Idr);
    let buffer = frame.buffer();
    // `Buffer::get_size()` returns `shiguredo_amf::ffi::amf_size`, a plain `usize` alias
    // (confirmed against source: `pub type amf_size = usize;`) — no fallible conversion
    // needed here, unlike `Plane::get_hpitch()`/etc. below (`amf_int32`).
    let len = buffer.get_size();
    let ptr = buffer.get_native().cast::<u8>();
    let bytes: &[u8] = if len == 0 || ptr.is_null() {
        &[]
    } else {
        // SAFETY: `buffer.get_native()` is AMD AMF's documented raw pointer to this
        // `Buffer` (`AMFBuffer`)'s encoded bitstream bytes; `get_size()` is that same
        // buffer's own reported length, read from the same object. `frame`/`buffer` stay
        // alive for this whole function (`buffer` borrows `frame`, not dropped until this
        // function returns after the slice is copied out via `Bytes::copy_from_slice`
        // below), so the slice never outlives its backing allocation.
        unsafe { std::slice::from_raw_parts(ptr, len) }
    };
    Packet {
        stream_id: 0,
        pts: meta.pts,
        dts: meta.pts,
        duration: meta.duration,
        is_keyframe,
        is_discard: false,
        payload: Bytes::copy_from_slice(bytes),
    }
}

/// AMD AMF H.264 encode session (CPU NV12 upload, `shiguredo_amf`-backed).
pub(crate) struct AmfSession {
    encoder: Encoder<PacketSink>,
    queue: Arc<Mutex<VecDeque<Result<Packet, EncodeError>>>>,
    info: StreamInfo,
    width: u32,
    height: u32,
    nv12_bytes: usize,
    /// Whether [`Self::open`] configured CBR rate control — gates [`Self::set_bitrate`],
    /// matching [`VideoEncoder::set_bitrate`]'s documented "CBR-only" contract.
    is_cbr: bool,
    flushed: bool,
}

impl AmfSession {
    /// Open according to [`VideoEncoderConfig::input`].
    pub(crate) fn open(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        validate(config)?;
        match config.input {
            VideoInputPreference::CpuUploadOk => Self::open_cpu(config),
            // No GPU-surface-import type confirmed to exist in `shiguredo_amf` at all —
            // see ADR-0002 § Scope.
            _ => Err(EncodeError::Unsupported),
        }
    }

    fn open_cpu(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        let (framerate_num, framerate_den) = codec::framerate_from_time_base(config.time_base);
        let codec_config = CodecConfig::H264(H264EncoderConfig { profile: None });
        let is_cbr = config.rate_control.is_some();
        let rate_control_mode = if is_cbr {
            RateControlMode::Cbr
        } else {
            RateControlMode::Cqp
        };

        let mut encoder_config = EncoderConfig::new(
            codec_config,
            config.width,
            config.height,
            FrameFormat::Nv12,
            framerate_num,
            framerate_den,
            rate_control_mode,
        );
        // Direct passthrough — see this module's doc comment on why no IDR-only fallback
        // is needed here, unlike `linux::vaapi`.
        encoder_config.gop_pic_size = u16::try_from(config.gop_size).ok();
        if let Some(rc) = config.rate_control {
            encoder_config.target_kbps = Some(codec::bps_to_kbps(rc.target_bitrate_bps));
            encoder_config.max_kbps = rc.vbv_buffer_size_bytes.map(codec::vbv_bytes_to_max_kbps);
        }

        let queue: Arc<Mutex<VecDeque<Result<Packet, EncodeError>>>> =
            Arc::new(Mutex::new(VecDeque::new()));
        // clone: Arc share — `handler` is moved into `Encoder::new` below (the callback
        // side); this session keeps its own handle on the same queue for `poll_packet` (the
        // pull side). See `PacketSink`'s doc comment.
        let handler = PacketSink {
            queue: Arc::clone(&queue),
        };

        let encoder = Encoder::new(encoder_config, handler).map_err(|_| EncodeError::Backend)?;
        let nv12_bytes = codec::nv12_size(config.width, config.height)?;

        Ok(Self {
            encoder,
            queue,
            info: stream_info_from(config),
            width: config.width,
            height: config.height,
            nv12_bytes,
            is_cbr,
            flushed: false,
        })
    }
}

impl VideoEncoder for AmfSession {
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
        if data.len() < self.nv12_bytes {
            return Err(EncodeError::InvalidInput);
        }

        let surface = self
            .encoder
            .alloc_surface()
            .map_err(|_| EncodeError::Backend)?;
        upload_cpu_nv12(&surface, data, self.width, self.height)?;

        surface.set_pts(frame.pts);
        let duration = i64::try_from(frame.duration).unwrap_or(i64::MAX);
        surface.set_duration(duration);

        let meta = FrameMeta {
            pts: frame.pts,
            duration: frame.duration,
        };
        let options = EncodeOptions::default();
        self.encoder
            .encode(surface, &options, meta)
            .map_err(|_| EncodeError::Backend)
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        let mut queue = self.queue.lock().map_err(|_| EncodeError::Backend)?;
        queue.pop_front().transpose()
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        if self.flushed {
            return Ok(());
        }
        self.flushed = true;
        // `finish()` drains `shiguredo_amf`'s internal pipeline synchronously — any
        // remaining `on_encoded` callbacks fire during this call, so the queue is fully
        // populated by the time this returns; the caller drains it via `poll_packet` per
        // this trait's documented two-step contract.
        self.encoder.finish().map_err(|_| EncodeError::Backend)
    }

    fn set_bitrate(&mut self, bitrate_bps: u32) -> Result<(), EncodeError> {
        if !self.is_cbr {
            return Err(EncodeError::Unsupported);
        }
        let params = ReconfigureParams {
            target_kbps: Some(codec::bps_to_kbps(bitrate_bps)),
            ..Default::default()
        };
        self.encoder
            .reconfigure(params)
            .map_err(|_| EncodeError::Backend)
    }
}

/// Copy CPU NV12 bytes into `surface`'s Y and UV planes (`Surface::get_plane` +
/// `Plane::get_native()` raw-pointer row writes) — a genuine CPU→driver copy, named to
/// match the sibling backends' `upload_cpu_nv12` cost-disclosure convention. `data` must be
/// tightly packed NV12 (`width * height` Y bytes followed by `width * height / 2`
/// interleaved UV bytes).
///
/// `AMF_PLANE_TYPE::AMF_PLANE_Y`/`AMF_PLANE_UV` (`shiguredo_amf::ffi::AMF_PLANE_TYPE`, a
/// `bindgen`-generated enum re-exported from that crate's internal `sys` module) — ADR-0002
/// flagged the exact variant names as unconfirmed after two docs.rs 404s; this
/// implementation confirmed them against the real crate source fetched by a WSL2 Linux
/// `x86_64` build (`~/.cargo/registry/src/.../shiguredo_amf-2026.3.0/build.rs`:
/// `AMF_PLANE_UNKNOWN = 0, AMF_PLANE_PACKED = 1, AMF_PLANE_Y = 2, AMF_PLANE_UV = 3,
/// AMF_PLANE_U = 4, AMF_PLANE_V = 5`), matching the standard AMD AMF SDK names guessed —
/// a real fact now, not a carried-forward guess.
fn upload_cpu_nv12(
    surface: &Surface,
    data: &[u8],
    width: u32,
    height: u32,
) -> Result<(), EncodeError> {
    let w = width as usize;
    let h = height as usize;
    let y_plane_bytes = w.checked_mul(h).ok_or(EncodeError::InvalidInput)?;
    let uv_rows = h / 2;

    let y_plane = surface
        .get_plane(AMF_PLANE_TYPE::AMF_PLANE_Y)
        .map_err(|_| EncodeError::Backend)?;
    write_plane_rows(&y_plane, data, 0, w, h)?;

    let uv_plane = surface
        .get_plane(AMF_PLANE_TYPE::AMF_PLANE_UV)
        .map_err(|_| EncodeError::Backend)?;
    write_plane_rows(&uv_plane, data, y_plane_bytes, w, uv_rows)?;

    Ok(())
}

/// Row-by-row pitch-aware copy of `rows` rows of `row_bytes` bytes each from
/// `data[src_offset..]` into `plane`'s native backing storage — mirrors
/// `linux::vaapi::upload_cpu_nv12`'s pitch/offset math, but through `Plane::get_native()`'s
/// raw pointer (AMF has no safe mapped-slice API, unlike VA-API's `Image::as_mut()`).
fn write_plane_rows(
    plane: &Plane,
    data: &[u8],
    src_offset: usize,
    row_bytes: usize,
    rows: usize,
) -> Result<(), EncodeError> {
    let hpitch = usize::try_from(plane.get_hpitch()).map_err(|_| EncodeError::Backend)?;
    let plane_width = usize::try_from(plane.get_width()).map_err(|_| EncodeError::Backend)?;
    let plane_height = usize::try_from(plane.get_height()).map_err(|_| EncodeError::Backend)?;
    if plane_width < row_bytes || plane_height < rows || hpitch < row_bytes {
        return Err(EncodeError::Backend);
    }
    let row_span = rows
        .checked_mul(row_bytes)
        .ok_or(EncodeError::InvalidInput)?;
    let src_end = src_offset
        .checked_add(row_span)
        .ok_or(EncodeError::InvalidInput)?;
    if data.len() < src_end {
        return Err(EncodeError::InvalidInput);
    }

    let base = plane.get_native().cast::<u8>();
    if base.is_null() {
        return Err(EncodeError::Backend);
    }

    for row in 0..rows {
        let src = src_offset + row * row_bytes;
        let dst_row_offset = row * hpitch;
        // SAFETY: `base` is AMD AMF's documented writable raw pointer to this `Plane`
        // (`AMFPlane`)'s CPU-mapped backing storage (`get_native()`), valid for at least
        // `get_vpitch() * get_hpitch()` bytes for the `Plane`/parent `Surface`'s lifetime
        // (`surface`, and thus `plane`, outlives this whole function call). `hpitch` is
        // this same plane's own reported row stride and `row < rows <= plane_height`
        // (bounds-checked above against `get_height()`), so `row * hpitch` stays within
        // the plane's mapped region; `row_bytes <= plane_width <= hpitch` (checked above)
        // keeps each row's write within that row's bounds, and rows never overlap
        // (`copy_nonoverlapping` with a source slice and a plane pointer that cannot
        // alias it). `data[src..src + row_bytes]` was bounds-checked via `src_end` above,
        // so the source read is an ordinary safe slice op — only the destination write
        // needs `unsafe`.
        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr().add(src),
                base.add(dst_row_offset),
                row_bytes,
            );
        }
    }
    Ok(())
}

fn validate(config: &VideoEncoderConfig) -> Result<(), EncodeError> {
    if !codec::is_supported_video_codec(config.codec) {
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
    // `0` is rejected rather than treated as "infinite GOP" — see
    // `VideoEncoderConfig::gop_size`'s doc contract.
    if config.gop_size == 0 {
        return Err(EncodeError::InvalidInput);
    }
    Ok(())
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
#[path = "session_tests.rs"]
mod tests;
