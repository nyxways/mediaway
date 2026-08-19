//! `VTDecompressionSession` CPU NV12 (`VideoRange`) readback decode session — H.264/HEVC/VP9/AV1.
//!
//! See [ADR-0001](../../../adr/apple/0001-videotoolbox-h264-cpu-out.md) for the original H.264
//! scope (general GOP via VideoToolbox-managed DPB + reorder, CPU NV12 `VideoRange` readback, one
//! SPS + one PPS + 4-byte AVCC length size only) and
//! [ADR-0002](../../../adr/apple/0002-videotoolbox-hevc-vp9-av1-decode.md) for the HEVC/VP9/AV1
//! multicodec expansion (HEVC mirrors H.264's in-band VPS/SPS/PPS shape; VP9/AV1 require a
//! container-supplied `vpcC`/`av1C` config record up front instead). The
//! zero-compile-verification caveat for this crate as authored carries over unchanged.
#![allow(unsafe_code)] // real `objc2-*` FFI calls — see this crate's `apple/mod.rs` doc comment

use std::collections::VecDeque;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};

use crate::{DecodeError, VideoDecoder, VideoDecoderConfig, VideoOutputPreference};
use mediaway_common::{
    Bytes, CodecKind, Packet, PixelFormat, Rational, StreamInfo, VideoFrame, VideoFrameStorage,
    VideoGeometry,
};

use iso_bmff::bitstream::avc::parse_avc_decoder_config;
use iso_bmff::bitstream::avc::to_avcc;
use iso_bmff::bitstream::hevc::{parse_hevc_decoder_config, to_hvcc};

use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_core_media::{
    CMBlockBuffer, CMBlockBufferCustomBlockSource, CMSampleBuffer, CMSampleTimingInfo, CMTime,
    CMTimeFlags, CMVideoFormatDescription, kCMBlockBufferCustomBlockSourceVersion,
};
use objc2_core_video::{
    CVImageBuffer, CVPixelBuffer, CVPixelBufferLockFlags, kCVPixelBufferPixelFormatTypeKey,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
};
use objc2_video_toolbox::{
    VTDecodeFrameFlags, VTDecodeInfoFlags, VTDecompressionOutputCallbackRecord,
    VTDecompressionSession,
};

use super::codec::{
    cmtime_value_from_ticks, copy_nv12_planes, duration_ticks_from_cmtime_value,
    is_supported_video_codec, raw_atom_key, requires_extra_data_at_open, ticks_from_cmtime_value,
    validate_hevc_parameter_sets, validate_parameter_sets,
};
use super::format_desc;

/// `OSStatus`/`CVReturn` "no error" value (both use the plain C convention `0 == success`) —
/// same reuse-for-both convention `mediaway-encoder::apple` already established.
const NO_ERROR: i32 = 0;

/// State shared between [`VideoToolboxVideoDecoder`] and the `VTDecompressionOutputCallback`,
/// which fires asynchronously on a VideoToolbox-internal thread — see ADR-0001 § Callback /
/// output-collection design. Bundles `time_base` alongside the output queue (needed by the
/// callback to convert VideoToolbox's returned `CMTime`s back to this crate's tick convention)
/// the same way the encoder ADR's own `SharedState` bundles fields beyond a bare queue.
struct SharedState {
    pending: Mutex<VecDeque<VideoFrame>>,
    time_base: Rational,
}

/// `VTDecompressionSession` decode session (CPU NV12 `VideoRange` readback, general GOP) —
/// H.264/HEVC (NAL-based, in-band parameter sets) or VP9/AV1 (raw, container-supplied config
/// record) depending on [`Self::codec`]. See ADR-0002 § Scope.
pub(crate) struct VideoToolboxVideoDecoder {
    /// `None` until the first parameter-set-bearing packet (or non-empty `extra_data` at
    /// `open()`) — lazy session creation, mirroring `linux::vaapi`'s identical "pipeline
    /// creation is lazy" decision (ADR-0001 § Session lifecycle). Always `Some` by the time
    /// `open()` returns for VP9/AV1 (see [`requires_extra_data_at_open`]).
    session: Option<CFRetained<VTDecompressionSession>>,
    /// The format description backing `session` — also `CMSampleBuffer::new`'s per-packet
    /// `format_description` argument (via `Deref` to `&CMFormatDescription`).
    video_format_desc: Option<CFRetained<CMVideoFormatDescription>>,
    shared: Arc<SharedState>,
    /// The extra `Arc::into_raw` strong count passed as `decompressionOutputRefCon` — `None`
    /// until a session exists (no callback can fire before then), reclaimed exactly once in
    /// `Drop`.
    refcon_ptr: Option<*const SharedState>,
    codec: CodecKind,
    info: StreamInfo,
    flushed: bool,
}

// SAFETY: `VTDecompressionSession`/`CFRetained` are Core Foundation objects, which Apple
// documents as safe to use from any thread as long as access is externally synchronized (this
// type's `&mut self` API surface already enforces that for the Rust side); the only shared
// mutable state reachable from another thread is `SharedState`, which uses `Mutex` internally.
#[allow(
    clippy::non_send_fields_in_send_ty,
    reason = "CFRetained<VTDecompressionSession> is a Core Foundation object — see the SAFETY comment above"
)]
unsafe impl Send for VideoToolboxVideoDecoder {}

impl VideoToolboxVideoDecoder {
    /// Open according to [`VideoDecoderConfig`].
    pub(crate) fn open(config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        validate(config)?;

        let shared = Arc::new(SharedState {
            pending: Mutex::new(VecDeque::new()),
            time_base: config.time_base,
        });

        let mut decoder = Self {
            session: None,
            video_format_desc: None,
            shared,
            refcon_ptr: None,
            codec: config.codec,
            info: stream_info_from(config),
            flushed: false,
        };

        if requires_extra_data_at_open(config.codec) {
            // VP9/AV1 have no in-band parameter-set NAL this backend can discover from the
            // first packet — the container-supplied config record must be here now (see
            // `codec::requires_extra_data_at_open`'s doc comment).
            if config.extra_data.is_empty() {
                return Err(DecodeError::Unsupported);
            }
            if config.width == 0 || config.height == 0 {
                return Err(DecodeError::InvalidInput);
            }
            let atom_key = raw_atom_key(config.codec).ok_or(DecodeError::Unsupported)?;
            let codec_type =
                format_desc::raw_codec_type(config.codec).ok_or(DecodeError::Unsupported)?;
            let width = i32::try_from(config.width).map_err(|_| DecodeError::InvalidInput)?;
            let height = i32::try_from(config.height).map_err(|_| DecodeError::InvalidInput)?;
            let fd =
                format_desc::create_raw(codec_type, width, height, atom_key, &config.extra_data)?;
            decoder.ensure_session(fd)?;
        } else if !config.extra_data.is_empty() {
            // `extra_data` non-empty at `open()` ⇒ build immediately (ADR-0001 § Session
            // lifecycle) — a malformed/unsupported record is a real error here, not a silent
            // fallback to in-band detection (unlike `linux::vaapi`'s best-effort `seed_params`,
            // which only ever softens genuine parse failures of otherwise-optional seed data).
            match config.codec {
                CodecKind::H264 => {
                    let avcc_config = parse_avc_decoder_config(&config.extra_data)
                        .ok_or(DecodeError::InvalidInput)?;
                    validate_parameter_sets(&avcc_config)?;
                    let fd = format_desc::create_h264(&avcc_config.sps[0], &avcc_config.pps[0])?;
                    decoder.ensure_session(fd)?;
                }
                CodecKind::Hevc => {
                    let hvcc_config = parse_hevc_decoder_config(&config.extra_data)
                        .ok_or(DecodeError::InvalidInput)?;
                    validate_hevc_parameter_sets(&hvcc_config)?;
                    let fd = format_desc::create_hevc(
                        &hvcc_config.vps[0],
                        &hvcc_config.sps[0],
                        &hvcc_config.pps[0],
                    )?;
                    decoder.ensure_session(fd)?;
                }
                // `validate()` (via `is_supported_video_codec`) already restricts `open()` to
                // H.264/HEVC/VP9/AV1, and VP9/AV1 took the `requires_extra_data_at_open` branch
                // above — nothing else reaches here.
                _ => return Err(DecodeError::Unsupported),
            }
        }
        Ok(decoder)
    }

    /// Create `VTDecompressionSession` from an already-built `format_desc` — codec-agnostic
    /// (H.264/HEVC/VP9/AV1 callers build `format_desc` differently, via `format_desc::
    /// create_{h264,hevc,raw}`, but session creation and the output callback wiring are
    /// identical for all of them). A no-op once a session exists — dynamic format
    /// renegotiation mid-session is unsupported this stage (ADR-0001 § Scope).
    fn ensure_session(
        &mut self,
        video_format_desc: CFRetained<CMVideoFormatDescription>,
    ) -> Result<(), DecodeError> {
        if self.session.is_some() {
            return Ok(());
        }
        let dest_attrs = destination_pixel_buffer_attributes();

        let refcon_ptr = Arc::into_raw(Arc::clone(&self.shared));
        let callback_record = VTDecompressionOutputCallbackRecord {
            decompressionOutputCallback: Some(decompression_output_callback),
            decompressionOutputRefCon: refcon_ptr.cast::<c_void>().cast_mut(),
        };

        let mut session_out: Option<CFRetained<VTDecompressionSession>> = None;
        // SAFETY: `video_format_desc` is a valid, just-created video format description;
        // `dest_attrs` is a valid `CFDictionary`; `callback_record.decompressionOutputCallback`
        // is a real `extern "C-unwind" fn` matching `VTDecompressionOutputCallback`'s exact
        // signature; `decompressionOutputRefCon` is a valid, live pointer for at least the
        // session's lifetime (reclaimed only in `Drop`, after `invalidate()`); `session_out`
        // starts `None`.
        let status = unsafe {
            VTDecompressionSession::new(
                None,
                &video_format_desc,
                None,
                Some(&dest_attrs),
                Some(&callback_record),
                &mut session_out,
            )
        };
        if status != NO_ERROR {
            // SAFETY: reclaims the extra strong count taken above; no callback can have fired
            // since the session was never successfully created.
            drop(unsafe { Arc::from_raw(refcon_ptr) });
            return Err(DecodeError::Backend);
        }
        let Some(session) = session_out else {
            // SAFETY: same reasoning as the failure branch above.
            drop(unsafe { Arc::from_raw(refcon_ptr) });
            return Err(DecodeError::Backend);
        };

        self.video_format_desc = Some(video_format_desc);
        self.session = Some(session);
        self.refcon_ptr = Some(refcon_ptr);
        Ok(())
    }
}

impl VideoDecoder for VideoToolboxVideoDecoder {
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

        // Per-codec framing (ADR-0002 § Byte framing): H.264/HEVC Annex-B input becomes
        // 4-byte-length-prefixed (and yields a fresh avcC/hvcC on the first parameter-set-
        // bearing packet, feeding lazy session creation below); already-framed input passes
        // through. VP9/AV1 are not NAL-based — the payload is fed to VideoToolbox byte-for-byte,
        // and the session already exists from `open()` (see `requires_extra_data_at_open`).
        let payload = match self.codec {
            CodecKind::H264 => {
                let avcc_out = to_avcc(&packet.payload);
                if self.session.is_none() {
                    let avcc_bytes = avcc_out.avcc.as_ref().ok_or(DecodeError::InvalidInput)?;
                    let avcc_config =
                        parse_avc_decoder_config(avcc_bytes).ok_or(DecodeError::InvalidInput)?;
                    validate_parameter_sets(&avcc_config)?;
                    let fd = format_desc::create_h264(&avcc_config.sps[0], &avcc_config.pps[0])?;
                    self.ensure_session(fd)?;
                }
                avcc_out.payload
            }
            CodecKind::Hevc => {
                let hvcc_out = to_hvcc(&packet.payload);
                if self.session.is_none() {
                    let hvcc_bytes = hvcc_out.hvcc.as_ref().ok_or(DecodeError::InvalidInput)?;
                    let hvcc_config =
                        parse_hevc_decoder_config(hvcc_bytes).ok_or(DecodeError::InvalidInput)?;
                    validate_hevc_parameter_sets(&hvcc_config)?;
                    let fd = format_desc::create_hevc(
                        &hvcc_config.vps[0],
                        &hvcc_config.sps[0],
                        &hvcc_config.pps[0],
                    )?;
                    self.ensure_session(fd)?;
                }
                hvcc_out.payload
            }
            CodecKind::Vp9 | CodecKind::Av1 => {
                if self.session.is_none() {
                    // `open()` requires `extra_data` up front for these codecs — reaching here
                    // without a session means `open()` never actually created one, which
                    // shouldn't happen (see `requires_extra_data_at_open`); treat it as a
                    // backend-state error rather than silently guessing a format description.
                    return Err(DecodeError::Backend);
                }
                Bytes::copy_from_slice(&packet.payload)
            }
            // `self.codec` was validated at `open()` (via `is_supported_video_codec`) to be one
            // of the four arms above — nothing else reaches here.
            _ => return Err(DecodeError::Unsupported),
        };

        let session = self.session.as_ref().ok_or(DecodeError::Backend)?;
        let format_desc = self
            .video_format_desc
            .as_ref()
            .ok_or(DecodeError::Backend)?;

        let block_buffer = create_block_buffer(&payload)?;
        let timing = build_timing_info(packet, self.shared.time_base);
        let sample_buffer = create_sample_buffer(&block_buffer, format_desc, &timing)?;

        let flags = VTDecodeFrameFlags::Frame_EnableAsynchronousDecompression
            | VTDecodeFrameFlags::Frame_EnableTemporalProcessing;
        // SAFETY: `session` is a valid, open session (guarded by the `Option` check above);
        // `sample_buffer` is a valid, just-created `CMSampleBuffer`; `source_frame_ref_con`/
        // `info_flags_out` are intentionally unused (`null_mut`/`None`) — this backend recovers
        // timing from the output callback's own `CMTime` parameters, not a threaded-through
        // refcon (ADR-0001 § Callback design).
        let status =
            unsafe { session.decode_frame(&sample_buffer, flags, std::ptr::null_mut(), None) };
        if status != NO_ERROR {
            return Err(DecodeError::Backend);
        }
        Ok(())
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        let mut pending = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(pending.pop_front())
    }

    fn flush(&mut self) -> Result<(), DecodeError> {
        if self.flushed {
            return Ok(());
        }
        self.flushed = true;
        let Some(session) = self.session.as_ref() else {
            // No session was ever created (stream ended before any SPS+PPS arrived) — nothing
            // to drain.
            return Ok(());
        };
        // SAFETY: `session` is a valid, still-open session (guarded by `self.flushed` and the
        // `Option` check above). `wait_for_asynchronous_frames`'s own doc comment: "Waits for
        // any and all outstanding asynchronous and delayed frames to complete... automatically
        // calls VTDecompressionSessionFinishDelayedFrames" — this backend's real, synchronous
        // drain point; `poll_frame` drains the shared queue afterward.
        let status = unsafe { session.wait_for_asynchronous_frames() };
        if status != NO_ERROR {
            return Err(DecodeError::Backend);
        }
        Ok(())
    }
}

impl Drop for VideoToolboxVideoDecoder {
    fn drop(&mut self) {
        if let Some(session) = self.session.as_ref() {
            // SAFETY: draining before invalidate — mitigates the unconfirmed
            // invalidate/callback-cutoff ordering guarantee ADR-0001 § Callback design flags as
            // an open question, same defensive posture the encoder backend already uses for its
            // own session type.
            let _ = unsafe { session.wait_for_asynchronous_frames() };
            // SAFETY: `session` is a valid, owned `CFRetained<VTDecompressionSession>` about to
            // be dropped; `invalidate()` is Apple's documented deterministic-teardown call,
            // meant to be followed by releasing the last reference (the `CFRetained` drop glue
            // below does that).
            unsafe { session.invalidate() };
        }
        if let Some(refcon_ptr) = self.refcon_ptr {
            // SAFETY: `refcon_ptr` is exactly the `Arc::into_raw` pointer taken in
            // `ensure_session` and handed to VideoToolbox as the callback's
            // `decompressionOutputRefCon`; reclaimed here exactly once, after `invalidate()`
            // above — no other code path frees it.
            drop(unsafe { Arc::from_raw(refcon_ptr) });
        }
    }
}

/// `VTDecompressionOutputCallback` — fires on a VideoToolbox-internal thread (possibly
/// asynchronously, decoupled from the `decode_frame` call that produced it, since
/// `kVTDecodeFrame_EnableTemporalProcessing` is always set — see ADR-0001 § Output ordering).
unsafe extern "C-unwind" fn decompression_output_callback(
    decompression_output_ref_con: *mut c_void,
    _source_frame_ref_con: *mut c_void,
    status: i32,
    _info_flags: VTDecodeInfoFlags,
    image_buffer: *mut CVImageBuffer,
    presentation_time_stamp: CMTime,
    presentation_duration: CMTime,
) {
    if status != NO_ERROR || image_buffer.is_null() {
        return;
    }
    // SAFETY: `decompression_output_ref_con` is exactly the `Arc::into_raw::<SharedState>`
    // pointer passed at session creation, reclaimed only in `Drop` after `invalidate()` — valid
    // for the whole lifetime any callback can fire in. Borrowed here, never re-owned
    // (`Arc::from_raw` is never called inside this function — see `Drop`'s own comment for the
    // single reclaim site).
    let shared = unsafe { &*(decompression_output_ref_con.cast::<SharedState>()) };
    // SAFETY: non-null (checked above); VideoToolbox guarantees a valid image buffer for the
    // duration of this callback invocation per `VTDecompressionOutputCallback`'s own contract.
    let image_buffer = unsafe { &*image_buffer };

    // Checked, not unchecked, downcast (ADR-0001 § Callback design) — `None` is a documented,
    // non-panicking skipped frame, not an assumption this crate cannot honestly make.
    let Some(pixel_buffer) = image_buffer.downcast_ref::<CVPixelBuffer>() else {
        return;
    };
    if pixel_buffer.pixel_format_type() != kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange {
        // VideoToolbox declined the requested NV12/VideoRange destination format — do not
        // misinterpret the bytes (ADR-0001 § Callback design).
        return;
    }

    // SAFETY: `pixel_buffer` is a valid, concrete `CVPixelBuffer`; this backend only ever reads
    // plane bytes below (never modifies the buffer), matching `ReadOnly`'s contract — a
    // symmetric lock/unlock pair follows (unlocked with the same flags before every return
    // path below).
    if unsafe { pixel_buffer.lock_base_address(CVPixelBufferLockFlags::ReadOnly) } != NO_ERROR {
        return;
    }

    let frame = build_frame(
        pixel_buffer.base_address_of_plane(0),
        pixel_buffer.bytes_per_row_of_plane(0),
        pixel_buffer.base_address_of_plane(1),
        pixel_buffer.bytes_per_row_of_plane(1),
        pixel_buffer.height_of_plane(1),
        pixel_buffer.width_of_plane(0),
        pixel_buffer.height_of_plane(0),
        presentation_time_stamp,
        presentation_duration,
        shared.time_base,
    );

    // SAFETY: matches the lock above — always unlock with the same flags used to lock.
    let _ = unsafe { pixel_buffer.unlock_base_address(CVPixelBufferLockFlags::ReadOnly) };

    let Some(frame) = frame else {
        return;
    };
    let mut pending = shared
        .pending
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    pending.push_back(frame);
}

/// Build a [`VideoFrame`] from a locked `CVPixelBuffer`'s NV12 plane pointers, or `None` on a
/// degenerate readback (zero dimensions / null plane base address) — never panics on bad
/// driver-reported geometry, mirroring `linux::vaapi`'s defensive NV12 readback discipline.
#[allow(
    clippy::too_many_arguments,
    reason = "raw CVPixelBuffer plane accessors read individually by the one call site above; grouping them into a struct would not simplify anything"
)]
fn build_frame(
    y_base: *mut c_void,
    y_stride: usize,
    uv_base: *mut c_void,
    uv_stride: usize,
    uv_height: usize,
    width: usize,
    height: usize,
    presentation_time_stamp: CMTime,
    presentation_duration: CMTime,
    time_base: Rational,
) -> Option<VideoFrame> {
    let width_u32 = u32::try_from(width).ok()?;
    let height_u32 = u32::try_from(height).ok()?;
    if width_u32 == 0 || height_u32 == 0 || y_base.is_null() || uv_base.is_null() {
        return None;
    }

    // SAFETY: `y_base` is a valid, locked `CVPixelBuffer` plane-0 base address (checked
    // non-null above), valid for `y_stride * height` bytes per
    // `CVPixelBufferGetBytesPerRowOfPlane`'s own contract, for as long as the buffer stays
    // locked (held by the caller for this whole scope).
    let y_plane = unsafe { std::slice::from_raw_parts(y_base.cast::<u8>(), y_stride * height) };
    // SAFETY: same reasoning as `y_plane` above, for plane 1 (UV).
    let uv_plane =
        unsafe { std::slice::from_raw_parts(uv_base.cast::<u8>(), uv_stride * uv_height) };

    let data = copy_nv12_planes(
        y_plane, y_stride, uv_plane, uv_stride, width_u32, height_u32,
    );

    let pts = ticks_from_cmtime_value(
        presentation_time_stamp.value,
        presentation_time_stamp.timescale,
        time_base,
    );
    let duration = duration_ticks_from_cmtime_value(
        presentation_duration.value,
        presentation_duration.timescale,
        time_base,
    );

    Some(VideoFrame {
        pts,
        duration,
        width: width_u32,
        height: height_u32,
        format: PixelFormat::Nv12,
        storage: VideoFrameStorage::Cpu { data },
    })
}

/// `destinationImageBufferAttributes` for `VTDecompressionSession::new` — forces NV12
/// (`kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange`) output (ADR-0001 § Session lifecycle):
/// **`VideoRange`, not `FullRange`** — decode consumes arbitrary third-party H.264 streams, for
/// which `VideoRange` is the more honest default absent VUI-based range detection (deferred,
/// ADR-0001 § Scope).
fn destination_pixel_buffer_attributes() -> CFRetained<CFDictionary<CFString, CFType>> {
    // Bit-pattern reinterpret of the `FourCharCode`/`OSType` pixel-format-type constant as a
    // signed 32-bit `CFNumber` — the standard CoreFoundation convention for a FourCC-valued
    // property (same reinterpretation `mediaway-encoder::apple` already uses for its own
    // pixel-format-type properties), not a magnitude cast.
    let value = i32::from_ne_bytes(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange.to_ne_bytes());
    let number = CFNumber::new_i32(value);
    let number_ct: &CFType = &number;
    // SAFETY: `kCVPixelBufferPixelFormatTypeKey` is a real, always-initialized CoreVideo
    // framework constant (an `extern "C"` static, safe to read for the process's lifetime).
    let key = unsafe { kCVPixelBufferPixelFormatTypeKey };
    CFDictionary::<CFString, CFType>::from_slices(&[key], &[number_ct])
}

/// Per-packet `CMSampleTimingInfo` from `Packet::{pts, dts, duration}` (ADR-0001 § Timestamps).
fn build_timing_info(packet: &Packet, time_base: Rational) -> CMSampleTimingInfo {
    let (pts_value, timescale) = cmtime_value_from_ticks(packet.pts, time_base);
    let (dts_value, _) = cmtime_value_from_ticks(packet.dts, time_base);
    let duration_ticks = i64::try_from(packet.duration).unwrap_or(i64::MAX);
    let (duration_value, _) = cmtime_value_from_ticks(duration_ticks, time_base);

    let cmtime = |value: i64| CMTime {
        value,
        timescale,
        flags: CMTimeFlags::Valid,
        epoch: 0,
    };

    CMSampleTimingInfo {
        duration: cmtime(duration_value),
        presentationTimeStamp: cmtime(pts_value),
        decodeTimeStamp: cmtime(dts_value),
    }
}

/// Wrap AVCC-framed packet bytes in a `CMBlockBuffer` for `CMSampleBuffer::new` (ADR-0001 §
/// Byte framing). The AVCC bytes are copied once into a heap-owned `Box<Vec<u8>>` — the one
/// real, named `memcpy` on this path (`to_avcc`'s own `Bytes` output is not directly
/// `CMBlockBuffer`-compatible, so a second copy is unavoidable here; flagged by the ADR as an
/// implementation-pass question, resolved by taking the straightforward owned-copy path rather
/// than an unproven raw-`Bytes`-pointer handoff) — freed exactly once by `free_avcc_block` when
/// VideoToolbox releases the block buffer.
fn create_block_buffer(payload: &Bytes) -> Result<CFRetained<CMBlockBuffer>, DecodeError> {
    // clone: `payload` is a shared `Bytes`; `CMBlockBuffer` needs a distinctly heap-owned,
    // exclusively-VideoToolbox-managed allocation it frees itself via `custom_block_source`'s
    // `FreeBlock` callback — see this function's own doc comment.
    let owned: Box<Vec<u8>> = Box::new(payload.to_vec());
    let len = owned.len();
    if len == 0 {
        return Err(DecodeError::InvalidInput);
    }
    let data_ptr = owned.as_ptr().cast_mut().cast::<c_void>();
    let refcon_ptr = Box::into_raw(owned).cast::<c_void>();

    let custom_block_source = CMBlockBufferCustomBlockSource {
        version: kCMBlockBufferCustomBlockSourceVersion,
        AllocateBlock: None,
        FreeBlock: Some(free_avcc_block),
        refCon: refcon_ptr,
    };

    let mut block_buffer_out: Option<CFRetained<CMBlockBuffer>> = None;
    // SAFETY: `data_ptr` points at `len` valid, exclusively-owned bytes for at least the
    // lifetime of the returned `CMBlockBuffer` (reclaimed exactly once by `free_avcc_block`,
    // which VideoToolbox calls when the block buffer's backing memory is freed);
    // `custom_block_source.FreeBlock` is a real `extern "C-unwind" fn` matching
    // `CMBlockBufferCustomBlockSource`'s exact `FreeBlock` signature; `block_buffer_out` starts
    // `None`.
    let status = unsafe {
        CMBlockBuffer::with_memory_block(
            None,
            data_ptr,
            len,
            None,
            Some(&custom_block_source),
            0,
            len,
            0,
            &mut block_buffer_out,
        )
    };
    if status != NO_ERROR {
        // SAFETY: creation failed before VideoToolbox could take ownership of the box (the
        // custom `FreeBlock` callback is never invoked on a `with_memory_block` failure) —
        // reclaim it here instead, the only path that would otherwise leak it.
        drop(unsafe { Box::from_raw(refcon_ptr.cast::<Vec<u8>>()) });
        return Err(DecodeError::Backend);
    }
    block_buffer_out.ok_or(DecodeError::Backend)
}

/// SAFETY: `ref_con` is exactly the `Box::into_raw(Box::new(Vec<u8>))` pointer
/// `create_block_buffer` passed as `custom_block_source.refCon` — `CMBlockBuffer` calls this
/// callback exactly once, when the block's backing memory is freed, and never otherwise touches
/// `ref_con`.
unsafe extern "C-unwind" fn free_avcc_block(
    ref_con: *mut c_void,
    _mem: NonNull<c_void>,
    _size: usize,
) {
    drop(unsafe { Box::from_raw(ref_con.cast::<Vec<u8>>()) });
}

/// One `CMSampleBuffer` per packet — this backend never batches multiple frames into one
/// buffer, matching `timing`'s single-entry contract (ADR-0001 § Timestamps).
fn create_sample_buffer(
    block_buffer: &CMBlockBuffer,
    format_desc: &CMVideoFormatDescription,
    timing: &CMSampleTimingInfo,
) -> Result<CFRetained<CMSampleBuffer>, DecodeError> {
    let mut sample_buffer_out: Option<CFRetained<CMSampleBuffer>> = None;
    // SAFETY: `block_buffer` is a valid, just-created `CMBlockBuffer`; `format_desc` is the
    // session's own retained format description (kept alive by `VideoToolboxVideoDecoder::
    // video_format_desc` for the whole session, `Deref`s to `&CMFormatDescription`); `timing`
    // is a valid stack value describing exactly the one sample in this buffer
    // (`num_sample_timing_entries: 1`, matching `CMSampleBufferCreate`'s "one entry applies to
    // all samples in this call" contract); `sample_buffer_out` starts `None`.
    let status = unsafe {
        CMSampleBuffer::new(
            None,
            Some(block_buffer),
            true,
            None,
            std::ptr::null_mut(),
            Some(format_desc),
            1,
            1,
            std::ptr::from_ref(timing),
            0,
            std::ptr::null(),
            &mut sample_buffer_out,
        )
    };
    if status != NO_ERROR {
        return Err(DecodeError::Backend);
    }
    sample_buffer_out.ok_or(DecodeError::Backend)
}

fn validate(config: &VideoDecoderConfig) -> Result<(), DecodeError> {
    if !is_supported_video_codec(config.codec) {
        return Err(DecodeError::Unsupported);
    }
    if config.output != VideoOutputPreference::CpuFramesOk {
        // Zero-Copy `CVPixelBuffer`/`IOSurface` output is deferred — see ADR-0001 § Scope.
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
        extra_data: config.extra_data.clone(), // clone: owned StreamInfo snapshot at open, mirrors linux::vaapi::h264's identical pattern
    }
}
