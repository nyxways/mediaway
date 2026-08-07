//! Opaque ADTS muxer handle and its C ABI functions.
//!
//! Dedicated shape, not a `MuxerState` variant: `adts::Muxer` has no track registration and
//! no `Open`/`Live` typestate (`adts::Muxer::new(sample_rate, channels)` is immediately
//! ready for `push_packet`) — see `adr/0004-ogg-adts-c-abi.md`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mediaway_common::{Bytes, Packet};
use mediaway_container::adts;

use crate::container::buffer::{borrow_slice, leak_boxed_slice};
use crate::container::status::MediawayStatus;
use crate::container::types::MediawayPacketView;

/// Opaque ADTS muxer handle (`mediaway_adts_muxer_t*` in the C header).
///
/// Thread-confined by convention: may be moved between threads, but must not be used from
/// two threads concurrently without external synchronization.
pub struct AdtsMuxerHandle {
    poisoned: bool,
    inner: adts::Muxer,
}

/// Open a mux session for `sample_rate` (must be a standard ADTS rate) / `channels`.
/// Immediately ready for [`mediaway_adts_muxer_push_packet`] — ADTS has no
/// track-registration step.
///
/// Returns null for a non-standard `sample_rate` ([`adts::Error::UnsupportedSampleRate`])
/// or if a panic was caught during construction — both collapse to null, matching
/// [`crate::container::mediaway_demuxer_create_for_format`]'s existing "unrecognized input
/// or panic" precedent (this constructor has no side channel for a status code).
#[unsafe(no_mangle)]
pub extern "C" fn mediaway_adts_muxer_create(
    sample_rate: u32,
    channels: u8,
) -> *mut AdtsMuxerHandle {
    let built = catch_unwind(AssertUnwindSafe(|| adts::Muxer::new(sample_rate, channels)));
    match built {
        Ok(Ok(inner)) => Box::into_raw(Box::new(AdtsMuxerHandle {
            poisoned: false,
            inner,
        })),
        Ok(Err(_)) | Err(_) => std::ptr::null_mut(),
    }
}

/// Append one AAC frame (raw, ADTS header added) from `packet`'s payload.
///
/// Fails with [`MediawayStatus::InvalidPacket`] if the payload is too large for ADTS's
/// 13-bit frame-length field ([`adts::Error::FrameTooLarge`]).
///
/// # Safety
///
/// `muxer` must be a live pointer returned by [`mediaway_adts_muxer_create`] and not yet
/// passed to [`mediaway_adts_muxer_close`]. `packet` must point to a valid
/// [`MediawayPacketView`] whose `payload`/`payload_len` describe a buffer valid for reads
/// for the duration of this call (or be null with length `0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_adts_muxer_push_packet(
    muxer: *mut AdtsMuxerHandle,
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
        handle
            .inner
            .push_packet(&packet)
            .map_err(MediawayStatus::from)
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

/// No-op — ADTS frames are independently appendable; nothing is buffered beyond what
/// [`mediaway_adts_muxer_poll_bytes`] already exposes. Exposed for shape parity with the
/// MP4/WebM/Ogg muxer APIs.
///
/// # Safety
///
/// `muxer` must be a live pointer returned by [`mediaway_adts_muxer_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_adts_muxer_flush(muxer: *mut AdtsMuxerHandle) -> MediawayStatus {
    if muxer.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `muxer` is a valid, live handle pointer (function contract).
    let handle = unsafe { &mut *muxer };
    if handle.poisoned {
        return MediawayStatus::HandlePoisoned;
    }
    handle.inner.flush();
    MediawayStatus::Ok
}

/// Drain whatever muxed ADTS bytes are ready right now into an owned buffer.
///
/// `*out_data == NULL && *out_len == 0` is a valid "nothing ready" result, not an error.
/// The returned buffer must be released with [`crate::container::mediaway_buffer_free`].
///
/// # Safety
///
/// `muxer` must be a live pointer returned by [`mediaway_adts_muxer_create`]. `out_data`
/// and `out_len` must be valid, writable, non-null out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_adts_muxer_poll_bytes(
    muxer: *mut AdtsMuxerHandle,
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
        let mut buf = Vec::new();
        handle.inner.poll_bytes(&mut buf);
        buf
    }));

    if let Ok(buf) = result {
        let (ptr, len) = leak_boxed_slice(buf);
        // SAFETY: `out_data`/`out_len` are checked non-null above (function contract).
        unsafe {
            out_data.write(ptr);
            out_len.write(len);
        }
        MediawayStatus::Ok
    } else {
        handle.poisoned = true;
        MediawayStatus::InternalPanic
    }
}

/// Close and free an ADTS muxer handle.
///
/// Always safe to call, including on a poisoned handle. In the unlikely event a panic
/// occurs while dropping the handle, the allocation is deliberately leaked rather than
/// double-handled (`adr/0001-mp4-mux-demux-c-abi.md` §7) — this would itself be a Mediaway
/// bug to fix if ever observed.
///
/// # Safety
///
/// `muxer` must be null or a pointer previously returned by [`mediaway_adts_muxer_create`]
/// and not already passed to this function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_adts_muxer_close(muxer: *mut AdtsMuxerHandle) {
    if muxer.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `muxer` is a valid, not-yet-freed handle pointer
        // (function contract).
        drop(unsafe { Box::from_raw(muxer) });
    }));
}
