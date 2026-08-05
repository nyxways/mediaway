//! Opus audio decode session C ABI — `mediaway_sw::opus::OpusDecoder` reachable
//! from C.
//!
//! Design: `adr/pipeline/0006-audio-decode-c-abi.md` — single-step open (the handle
//! *is* the decoder, same shape as [`crate::pipeline::decoder`]'s video surface),
//! `poisoned`-guarded (`push_packet`/`poll_frame` are repeated-call APIs). Wraps the
//! concrete `mediaway_sw::opus::OpusDecoder` directly rather than a `Box<dyn
//! AudioDecoder>` — no `AudioDecoder` trait exists yet in `mediaway-decoder`
//! (`mediaway-sw`'s own opus module docs), so a trait object here would abstract
//! over a backend set of exactly one. An empty `payload` in
//! [`mediaway_audio_decode_session_push_packet`] is Opus's packet-loss-concealment
//! hint, passed straight through — the same contract
//! `mediaway_sw::opus::OpusDecoder::push_packet` already documents.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mediaway_common::{Bytes, Packet};
use mediaway_sw::opus::config::OpusDecoderConfig;
use mediaway_sw::opus::decoder::OpusDecoder;

use crate::pipeline::buffer::{borrow_slice, leak_boxed_slice, reclaim_boxed_slice};
use crate::pipeline::status::MediawayPipelineStatus;
use crate::pipeline::types::{
    MediawayAudioDecodeConfig, MediawayDecodePacketView, MediawayDecodedAudioFrame,
    MediawayPipelineCodecKind,
};

/// Opaque audio decode-session handle (`mediaway_audio_decode_session_t*` in the C
/// header). See module docs for why this wraps `OpusDecoder` directly instead of a
/// trait object.
///
/// Thread-confined by convention, same as every other handle in this crate: may be
/// moved between threads, but must not be used from two threads concurrently
/// without external synchronization.
pub struct AudioDecodeSessionHandle {
    poisoned: bool,
    inner: OpusDecoder,
}

/// Open an Opus decode session for `config`.
///
/// Three outcomes: (1) `Ok` — builds the handle, writes it to `*out_session`; (2) a
/// normal `Err` (e.g. `config.codec != Opus` → `Unsupported`) — no handle exists,
/// `*out_session` is set to `NULL`, the matching status is returned; (3) a caught
/// panic — same `NULL`/[`MediawayPipelineStatus::InternalPanic`] shape as (2).
///
/// # Safety
///
/// `config` must be a valid, readable [`MediawayAudioDecodeConfig`] pointer.
/// `out_session` must be a valid, writable, non-null out-parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_audio_decode_session_open(
    config: *const MediawayAudioDecodeConfig,
    out_session: *mut *mut AudioDecodeSessionHandle,
) -> MediawayPipelineStatus {
    if config.is_null() || out_session.is_null() {
        return MediawayPipelineStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `config` is valid for reads (function contract).
    let config = unsafe { *config };
    // SAFETY: `out_session` is checked non-null above; caller guarantees it is
    // writable (function contract).
    unsafe { out_session.write(std::ptr::null_mut()) };

    if config.codec != MediawayPipelineCodecKind::Opus {
        return MediawayPipelineStatus::Unsupported; // only Opus today
    }
    if config.sample_rate == 0 || config.channels == 0 {
        return MediawayPipelineStatus::InvalidInput;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        let sw_config =
            OpusDecoderConfig::new(config.sample_rate, config.channels, config.time_base.into());
        OpusDecoder::open(&sw_config)
    }));

    match result {
        Ok(Ok(decoder)) => {
            let handle = Box::new(AudioDecodeSessionHandle {
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

/// Push one compressed Opus packet. May produce zero or more frames (drain via
/// [`mediaway_audio_decode_session_poll_frame`]).
///
/// `packet`'s `payload` is a caller-owned borrow, valid for the call only — the
/// core copies it synchronously. An empty payload (`payload == NULL` or
/// `payload_len == 0`) is Opus's packet-loss-concealment hint for a lost frame, not
/// an error.
///
/// # Safety
///
/// `session` must be a valid, live handle pointer. `packet` must be a valid,
/// readable pointer whose `payload` (when `payload_len > 0`) points to
/// `payload_len` readable bytes, both valid for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_audio_decode_session_push_packet(
    session: *mut AudioDecodeSessionHandle,
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
    // (function contract). `payload == NULL && payload_len == 0` is a legal Opus
    // packet-loss-concealment hint — `borrow_slice` already returns `Some(&[])` for
    // that exact pair, not `None` (only a mismatched null-with-nonzero-length pair
    // is rejected below).
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

/// Pull the next decoded PCM frame, if any is ready.
///
/// `*out_has_frame == false` is a valid "nothing ready" result, not an error. When
/// `true`, release `*out_frame` with [`mediaway_decoded_audio_frame_free`].
///
/// # Safety
///
/// `session` must be a valid, live handle pointer. `out_frame`/`out_has_frame` must
/// be valid, writable, non-null out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_audio_decode_session_poll_frame(
    session: *mut AudioDecodeSessionHandle,
    out_frame: *mut MediawayDecodedAudioFrame,
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
        let (data_ptr, data_len) = leak_boxed_slice(frame.data.to_vec());
        Ok(Some(MediawayDecodedAudioFrame {
            pts: frame.pts,
            duration: frame.duration,
            sample_rate: frame.sample_rate,
            channels: frame.channels,
            sample_format: frame.format.into(),
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

/// Signal end-of-input.
///
/// `push_packet` always decodes and enqueues synchronously
/// (`mediaway_sw::opus::OpusDecoder::flush`'s own doc), so this only marks the
/// session closed — call [`mediaway_audio_decode_session_poll_frame`] beforehand to
/// drain any pending frame.
///
/// # Safety
///
/// `session` must be a valid, live handle pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_audio_decode_session_flush(
    session: *mut AudioDecodeSessionHandle,
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

/// Close and free an audio decode-session handle. Always safe to call, including on
/// a poisoned handle or with `session == NULL`.
///
/// # Safety
///
/// `session` must be null or a pointer previously returned by
/// [`mediaway_audio_decode_session_open`] and not already closed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_audio_decode_session_close(
    session: *mut AudioDecodeSessionHandle,
) {
    if session.is_null() {
        return;
    }
    // A panic during drop is deliberately swallowed and the allocation leaked — same
    // reasoning as `mediaway_decode_session_close`.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `session` is a valid, not-yet-closed handle
        // pointer (function contract).
        drop(unsafe { Box::from_raw(session) });
    }));
}

/// Free a frame returned by [`mediaway_audio_decode_session_poll_frame`]. Nulls
/// `data`/`data_len` afterward, making a double-free a visible no-op. Always safe
/// to call, including with `frame == NULL`.
///
/// # Safety
///
/// `frame` must be null or a valid, writable pointer to a frame previously written
/// by `mediaway_audio_decode_session_poll_frame`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_decoded_audio_frame_free(frame: *mut MediawayDecodedAudioFrame) {
    if frame.is_null() {
        return;
    }
    // SAFETY: caller guarantees `frame` is a valid, writable pointer (function
    // contract).
    let frame = unsafe { &mut *frame };
    // SAFETY: `frame.data`/`frame.data_len` were produced by `leak_boxed_slice` via
    // `mediaway_audio_decode_session_poll_frame` (function contract).
    unsafe { reclaim_boxed_slice(frame.data, frame.data_len) };
    frame.data = std::ptr::null_mut();
    frame.data_len = 0;
}
