//! Opaque decode-session handle and its C ABI functions.
//!
//! Design: `adr/0004-auto-decode-c-abi.md` — single-step open (the handle *is* the
//! decoder, no muxer to wire), `poisoned`-guarded (`push_packet`/`poll_frame` are
//! repeated-call APIs), CPU-output-only v1.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mediaway::platform::AutoDecoder;
use mediaway_common::{Bytes, Packet, VideoFrameStorage};
use mediaway_decoder::{VideoDecoder, VideoDecoderConfig, VideoOutputPreference};

use crate::pipeline::buffer::{borrow_slice, leak_boxed_slice, reclaim_boxed_slice};
use crate::pipeline::status::MediawayPipelineStatus;
use crate::pipeline::types::{
    MediawayAutoVideoDecodeConfig, MediawayDecodePacketView, MediawayDecodedVideoFrame,
};

/// Opaque decode-session handle (`mediaway_decode_session_t*` in the C header).
///
/// The handle *is* the decoder — no intermediate handle, no consumption trap (there
/// is no muxer to wire, unlike `mediaway_auto_encoder_t`/`mediaway_encode_session_t`;
/// same reasoning `adr/0003-auto-audio-encode-c-abi.md` already used for audio
/// encode). `push_packet`/`poll_frame` are repeated-call APIs, so this needs the same
/// `poisoned` guard as `MuxerHandle`/`DemuxerHandle`/`EncodeSessionHandle` — unlike
/// `AutoEncoderHandle`/`AudioEncodeSessionHandle`'s no-`poisoned`-flag shape.
///
/// Thread-confined by convention: may be moved between threads, but must not be used
/// from two threads concurrently without external synchronization.
pub struct DecodeSessionHandle {
    poisoned: bool,
    inner: Box<dyn VideoDecoder>,
}

/// Open the best available video decoder for `config` on the current platform.
///
/// `config.extra_data` (AVCC / SPS-PPS codec config) is read and copied
/// synchronously during this call — see `adr/0004-auto-decode-c-abi.md` §1 for why
/// it must be supplied at open time rather than via the first pushed packet.
///
/// Three outcomes: (1) `Ok` — builds the handle, writes it to `*out_session`; (2) a
/// normal `Err` (e.g. [`mediaway_decoder::DecodeError::NoBackend`]) — no handle
/// exists, `*out_session` is set to `NULL`, the matching status is returned; (3) a
/// caught panic — same `NULL`/[`MediawayPipelineStatus::InternalPanic`] shape as (2).
///
/// # Safety
///
/// `config` must be a valid, readable [`MediawayAutoVideoDecodeConfig`] pointer
/// whose `extra_data` (when `extra_data_len > 0`) points to `extra_data_len`
/// readable bytes, valid for the duration of this call. `out_session` must be a
/// valid, writable, non-null out-parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_decode_session_open(
    config: *const MediawayAutoVideoDecodeConfig,
    out_session: *mut *mut DecodeSessionHandle,
) -> MediawayPipelineStatus {
    if config.is_null() || out_session.is_null() {
        return MediawayPipelineStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `config` is valid for reads (function contract).
    let config = unsafe { *config };
    // SAFETY: `out_session` is checked non-null above; caller guarantees it is
    // writable (function contract).
    unsafe { out_session.write(std::ptr::null_mut()) };
    // SAFETY: caller guarantees `config.extra_data`/`config.extra_data_len` describe
    // a buffer valid for this call (function contract).
    let Some(extra_data) = (unsafe { borrow_slice(config.extra_data, config.extra_data_len) })
    else {
        return MediawayPipelineStatus::InvalidArgument;
    };

    let result = catch_unwind(AssertUnwindSafe(|| {
        let rust_config = VideoDecoderConfig {
            codec: config.codec.into(),
            width: config.width,
            height: config.height,
            time_base: config.time_base.into(),
            pixel_format: config.pixel_format.into(),
            // GPU output stays deferred this pass (`adr/0004-auto-decode-c-abi.md` §1).
            output: VideoOutputPreference::CpuFramesOk,
            gpu_device: None,
            extra_data: Bytes::copy_from_slice(extra_data),
        };
        AutoDecoder::open(&rust_config)
    }));

    match result {
        Ok(Ok(decoder)) => {
            let handle = Box::new(DecodeSessionHandle {
                poisoned: false,
                inner: decoder,
            });
            // SAFETY: `out_session` is checked non-null above (function contract).
            unsafe { out_session.write(Box::into_raw(handle)) };
            MediawayPipelineStatus::Ok
        }
        Ok(Err(err)) => err.into(),
        Err(_) => MediawayPipelineStatus::InternalPanic,
    }
}

/// Push one compressed packet. May produce zero or more frames (drain via
/// [`mediaway_decode_session_poll_frame`]).
///
/// `packet`'s `payload` is a caller-owned borrow, valid for the call only — the
/// core copies it synchronously.
///
/// # Safety
///
/// `session` must be a valid, live handle pointer. `packet` must be a valid,
/// readable pointer whose `payload` (when `payload_len > 0`) points to
/// `payload_len` readable bytes, both valid for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_decode_session_push_packet(
    session: *mut DecodeSessionHandle,
    packet: *const MediawayDecodePacketView,
) -> MediawayPipelineStatus {
    if session.is_null() || packet.is_null() {
        return MediawayPipelineStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `session` is a valid, live handle pointer (function
    // contract).
    let handle = unsafe { &mut *session };
    if handle.poisoned {
        return MediawayPipelineStatus::HandlePoisoned;
    }
    // SAFETY: caller guarantees `packet` is valid for reads (function contract).
    let view = unsafe { *packet };
    // SAFETY: `view.payload`/`view.payload_len` describe a buffer valid for this call
    // (function contract).
    let Some(payload) = (unsafe { borrow_slice(view.payload, view.payload_len) }) else {
        return MediawayPipelineStatus::InvalidArgument;
    };

    let result = catch_unwind(AssertUnwindSafe(|| {
        let packet = Packet {
            stream_id: view.stream_id,
            pts: view.pts,
            dts: view.dts,
            duration: view.duration,
            is_keyframe: view.is_keyframe,
            is_discard: view.is_discard,
            payload: Bytes::copy_from_slice(payload),
        };
        handle
            .inner
            .push_packet(&packet)
            .map_err(MediawayPipelineStatus::from)
    }));

    match result {
        Ok(Ok(())) => MediawayPipelineStatus::Ok,
        Ok(Err(status)) => status,
        Err(_) => {
            handle.poisoned = true;
            MediawayPipelineStatus::InternalPanic
        }
    }
}

/// Pull the next decoded frame, if any is ready.
///
/// `*out_has_frame == false` is a valid "nothing ready" result, not an error. When
/// `true`, release `*out_frame` with [`mediaway_decoded_video_frame_free`].
///
/// # Safety
///
/// `session` must be a valid, live handle pointer. `out_frame`/`out_has_frame` must
/// be valid, writable, non-null out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_decode_session_poll_frame(
    session: *mut DecodeSessionHandle,
    out_frame: *mut MediawayDecodedVideoFrame,
    out_has_frame: *mut bool,
) -> MediawayPipelineStatus {
    if session.is_null() || out_frame.is_null() || out_has_frame.is_null() {
        return MediawayPipelineStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `session` is a valid, live handle pointer (function
    // contract).
    let handle = unsafe { &mut *session };
    if handle.poisoned {
        return MediawayPipelineStatus::HandlePoisoned;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        let maybe_frame = handle
            .inner
            .poll_frame()
            .map_err(MediawayPipelineStatus::from)?;
        let Some(frame) = maybe_frame else {
            return Ok(None);
        };
        let VideoFrameStorage::Cpu { data } = frame.storage else {
            // GPU output is deferred this pass (`adr/0004-auto-decode-c-abi.md` §1) —
            // a backend that produced one anyway (should not happen with
            // `CpuFramesOk` requested) surfaces as Unsupported rather than silently
            // dropping the frame.
            return Err(MediawayPipelineStatus::Unsupported);
        };
        let (data_ptr, data_len) = leak_boxed_slice(data.to_vec());
        Ok(Some(MediawayDecodedVideoFrame {
            pts: frame.pts,
            duration: frame.duration,
            width: frame.width,
            height: frame.height,
            pixel_format: frame.format.into(),
            data: data_ptr,
            data_len,
        }))
    }));

    match result {
        Ok(Ok(Some(frame))) => {
            // SAFETY: `out_frame`/`out_has_frame` are checked non-null above
            // (function contract).
            unsafe {
                out_frame.write(frame);
                out_has_frame.write(true);
            }
            MediawayPipelineStatus::Ok
        }
        Ok(Ok(None)) => {
            // SAFETY: `out_has_frame` is checked non-null above (function contract).
            unsafe { out_has_frame.write(false) };
            MediawayPipelineStatus::Ok
        }
        Ok(Err(status)) => status,
        Err(_) => {
            handle.poisoned = true;
            MediawayPipelineStatus::InternalPanic
        }
    }
}

/// Signal end-of-input; drain remaining frames with
/// [`mediaway_decode_session_poll_frame`] afterward.
///
/// # Safety
///
/// `session` must be a valid, live handle pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_decode_session_flush(
    session: *mut DecodeSessionHandle,
) -> MediawayPipelineStatus {
    if session.is_null() {
        return MediawayPipelineStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `session` is a valid, live handle pointer (function
    // contract).
    let handle = unsafe { &mut *session };
    if handle.poisoned {
        return MediawayPipelineStatus::HandlePoisoned;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        handle.inner.flush().map_err(MediawayPipelineStatus::from)
    }));

    match result {
        Ok(Ok(())) => MediawayPipelineStatus::Ok,
        Ok(Err(status)) => status,
        Err(_) => {
            handle.poisoned = true;
            MediawayPipelineStatus::InternalPanic
        }
    }
}

/// Close and free a decode-session handle. Always safe to call, including on a
/// poisoned handle or with `session == NULL`.
///
/// # Safety
///
/// `session` must be null or a pointer previously returned by
/// [`mediaway_decode_session_open`] and not already closed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_decode_session_close(session: *mut DecodeSessionHandle) {
    if session.is_null() {
        return;
    }
    // A panic during drop is deliberately swallowed and the allocation leaked — same
    // reasoning as `mediaway_muxer_close` (`adr/0001-auto-encode-c-abi.md` §7).
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `session` is a valid, not-yet-closed handle
        // pointer (function contract).
        drop(unsafe { Box::from_raw(session) });
    }));
}

/// Free a frame returned by [`mediaway_decode_session_poll_frame`]. Nulls
/// `data`/`data_len` afterward, making a double-free a visible no-op. Always safe
/// to call, including with `frame == NULL`.
///
/// # Safety
///
/// `frame` must be null or a valid, writable pointer to a frame previously written
/// by `mediaway_decode_session_poll_frame`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_decoded_video_frame_free(frame: *mut MediawayDecodedVideoFrame) {
    if frame.is_null() {
        return;
    }
    // SAFETY: caller guarantees `frame` is a valid, writable pointer (function
    // contract).
    let frame = unsafe { &mut *frame };
    // SAFETY: `frame.data`/`frame.data_len` were produced by `leak_boxed_slice` via
    // `mediaway_decode_session_poll_frame` (function contract).
    unsafe { reclaim_boxed_slice(frame.data, frame.data_len) };
    frame.data = std::ptr::null_mut();
    frame.data_len = 0;
}
