//! Opaque WAV (RIFF/WAVE PCM) muxer handle and its C ABI functions.
//!
//! Dedicated shape, not a `MuxerState` variant: `wav::Muxer::push_packet` is infallible (no
//! `Result`) and `wav::Muxer::finish` **consumes `self` by value** — RIFF chunk sizes must
//! be known up front, so the whole byte stream is only ever produced once, at the end, not
//! incrementally via `poll_bytes`. The handle holds `Option<wav::Muxer>` so
//! [`mediaway_wav_muxer_finish`] can [`Option::take`] it; a `None` inner value (already
//! finished) fails subsequent `push_packet`/`finish` calls with
//! [`MediawayStatus::InvalidState`] rather than panicking. See `adr/0008-wav-c-abi.md`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mediaway_common::{Bytes, Packet};
use mediaway_container::wav;
use riff_wave_core::{SampleFormat, WaveFormat};

use crate::container::buffer::{borrow_slice, leak_boxed_slice};
use crate::container::status::MediawayStatus;
use crate::container::types::{MediawayPacketView, MediawayWavSampleFormat, MediawayWaveFormat};

/// Opaque WAV muxer handle (`mediaway_wav_muxer_t*` in the C header).
///
/// Thread-confined by convention: may be moved between threads, but must not be used from
/// two threads concurrently without external synchronization.
pub struct WavMuxerHandle {
    poisoned: bool,
    /// `None` once [`mediaway_wav_muxer_finish`] has consumed it.
    inner: Option<wav::Muxer>,
}

const fn to_sample_format(format: MediawayWavSampleFormat) -> SampleFormat {
    match format {
        MediawayWavSampleFormat::Pcm => SampleFormat::Pcm,
        MediawayWavSampleFormat::Float => SampleFormat::Float,
    }
}

/// Start an integer-PCM mux session.
///
/// Returns null only if a panic was caught during construction (defensive; `wav::Muxer::new`
/// is a `const fn` field-init, so this should not trigger in practice).
#[unsafe(no_mangle)]
pub extern "C" fn mediaway_wav_muxer_create(
    sample_rate: u32,
    channels: u16,
    bits_per_sample: u16,
) -> *mut WavMuxerHandle {
    let built = catch_unwind(AssertUnwindSafe(|| WavMuxerHandle {
        poisoned: false,
        inner: Some(wav::Muxer::new(sample_rate, channels, bits_per_sample)),
    }));
    built.map_or(std::ptr::null_mut(), |handle| {
        Box::into_raw(Box::new(handle))
    })
}

/// Start a mux session for an explicit format (e.g. IEEE float PCM).
///
/// Returns null only if a panic was caught during construction, or if `format` is null.
///
/// # Safety
///
/// `format` must point to a valid [`MediawayWaveFormat`], valid for reads for the duration
/// of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_wav_muxer_create_with_format(
    format: *const MediawayWaveFormat,
) -> *mut WavMuxerHandle {
    if format.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: caller guarantees `format` is valid for reads (function contract).
    let format = unsafe { *format };
    let format = WaveFormat {
        sample_format: to_sample_format(format.sample_format),
        channels: format.channels,
        sample_rate: format.sample_rate,
        bits_per_sample: format.bits_per_sample,
    };

    let built = catch_unwind(AssertUnwindSafe(|| WavMuxerHandle {
        poisoned: false,
        inner: Some(wav::Muxer::with_format(format)),
    }));
    built.map_or(std::ptr::null_mut(), |handle| {
        Box::into_raw(Box::new(handle))
    })
}

/// Append raw interleaved PCM bytes (already encoded per the session's format).
///
/// Always succeeds (no per-call validation — `wav::Muxer::push_packet` is infallible)
/// unless the muxer already finished, which fails with [`MediawayStatus::InvalidState`].
///
/// # Safety
///
/// `muxer` must be a live pointer returned by [`mediaway_wav_muxer_create`]/
/// [`mediaway_wav_muxer_create_with_format`] and not yet passed to
/// [`mediaway_wav_muxer_finish`]/[`mediaway_wav_muxer_close`]. `packet` must point to a
/// valid [`MediawayPacketView`] whose `payload`/`payload_len` describe a buffer valid for
/// reads for the duration of this call (or be null with length `0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_wav_muxer_push_packet(
    muxer: *mut WavMuxerHandle,
    packet: *const MediawayPacketView,
) -> MediawayStatus {
    if muxer.is_null() || packet.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `muxer` is a valid, live handle pointer (function contract).
    let handle = unsafe { &mut *muxer };
    if handle.poisoned {
        return MediawayStatus::HandlePoisoned;
    }
    // SAFETY: caller guarantees `packet` is valid for reads (function contract).
    let view = unsafe { *packet };
    // SAFETY: `view.payload`/`view.payload_len` describe a buffer valid for this call
    // (function contract).
    let Some(payload) = (unsafe { borrow_slice(view.payload, view.payload_len) }) else {
        return MediawayStatus::InvalidArgument;
    };

    let packet = Packet {
        stream_id: view.stream_id,
        pts: view.pts,
        dts: view.dts,
        duration: view.duration,
        is_keyframe: view.is_keyframe,
        is_discard: view.is_discard,
        payload: Bytes::copy_from_slice(payload),
    };
    let result = catch_unwind(AssertUnwindSafe(|| {
        let Some(inner) = handle.inner.as_mut() else {
            return Err(MediawayStatus::InvalidState);
        };
        inner.push_packet(&packet);
        Ok(())
    }));

    match result {
        Ok(Ok(())) => MediawayStatus::Ok,
        Ok(Err(status)) => status,
        Err(_) => {
            handle.poisoned = true;
            MediawayStatus::InternalPanic
        }
    }
}

/// Finalize the mux session and return the complete RIFF/WAVE byte stream.
///
/// Consumes the muxer's internal state — a second call fails with
/// [`MediawayStatus::InvalidState`] rather than re-finalizing. The handle itself must still
/// be released with [`mediaway_wav_muxer_close`] afterward. The returned buffer must be
/// released with [`crate::container::mediaway_buffer_free`].
///
/// # Safety
///
/// `muxer` must be a live pointer returned by [`mediaway_wav_muxer_create`]/
/// [`mediaway_wav_muxer_create_with_format`]. `out_data` and `out_len` must be valid,
/// writable, non-null out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_wav_muxer_finish(
    muxer: *mut WavMuxerHandle,
    out_data: *mut *mut u8,
    out_len: *mut usize,
) -> MediawayStatus {
    if muxer.is_null() || out_data.is_null() || out_len.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `muxer` is a valid, live handle pointer (function contract).
    let handle = unsafe { &mut *muxer };
    if handle.poisoned {
        return MediawayStatus::HandlePoisoned;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        let Some(inner) = handle.inner.take() else {
            return Err(MediawayStatus::InvalidState);
        };
        Ok(inner.finish())
    }));

    match result {
        Ok(Ok(buf)) => {
            let (ptr, len) = leak_boxed_slice(buf);
            // SAFETY: `out_data`/`out_len` are checked non-null above (function contract).
            unsafe {
                out_data.write(ptr);
                out_len.write(len);
            }
            MediawayStatus::Ok
        }
        Ok(Err(status)) => status,
        Err(_) => {
            handle.poisoned = true;
            MediawayStatus::InternalPanic
        }
    }
}

/// Close and free a WAV muxer handle.
///
/// Always safe to call, including on a poisoned handle or one that never called
/// [`mediaway_wav_muxer_finish`] (the buffered PCM bytes are simply dropped). In the
/// unlikely event a panic occurs while dropping the handle, the allocation is deliberately
/// leaked rather than double-handled (`adr/0001-mp4-mux-demux-c-abi.md` §7) — this would
/// itself be a Mediaway bug to fix if ever observed.
///
/// # Safety
///
/// `muxer` must be null or a pointer previously returned by [`mediaway_wav_muxer_create`]/
/// [`mediaway_wav_muxer_create_with_format`] and not already passed to this function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_wav_muxer_close(muxer: *mut WavMuxerHandle) {
    if muxer.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `muxer` is a valid, not-yet-freed handle pointer
        // (function contract).
        drop(unsafe { Box::from_raw(muxer) });
    }));
}
