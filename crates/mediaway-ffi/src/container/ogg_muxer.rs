//! Opaque Ogg muxer handle and its C ABI functions.
//!
//! Dedicated shape, not a `MuxerState` variant: `ogg::Muxer` has no track registration and
//! no `Open`/`Live` typestate (`ogg::Muxer::new(serial)` is immediately ready for
//! `push_packet`) — see `adr/0004-ogg-adts-c-abi.md`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mediaway_common::{Bytes, Packet};
use mediaway_container::ogg;

use crate::container::buffer::{borrow_slice, leak_boxed_slice};
use crate::container::status::MediawayStatus;
use crate::container::types::MediawayPacketView;

/// Opaque Ogg muxer handle (`mediaway_ogg_muxer_t*` in the C header).
///
/// Thread-confined by convention: may be moved between threads, but must not be used from
/// two threads concurrently without external synchronization.
pub struct OggMuxerHandle {
    poisoned: bool,
    inner: ogg::Muxer,
}

/// Open a mux session for logical bitstream `serial`. Immediately ready for
/// [`mediaway_ogg_muxer_push_packet`] — Ogg has no track-registration step.
///
/// Returns null only if a panic was caught during construction (defensive; `ogg::Muxer::new`
/// is a `const fn` field-init, so this should not trigger in practice).
#[unsafe(no_mangle)]
pub extern "C" fn mediaway_ogg_muxer_create(serial: u32) -> *mut OggMuxerHandle {
    let built = catch_unwind(AssertUnwindSafe(|| OggMuxerHandle {
        poisoned: false,
        inner: ogg::Muxer::new(serial),
    }));
    built.map_or(std::ptr::null_mut(), |handle| {
        Box::into_raw(Box::new(handle))
    })
}

/// Write one Ogg page containing `packet`'s payload. `packet.pts` becomes the page's
/// `granule_position`; `packet.is_discard` becomes the page's `eos` flag.
///
/// Fails with [`MediawayStatus::InvalidData`] when the payload exceeds a single Ogg page's
/// capacity (this mux always emits one page per packet — see the `ogg-core` crate docs).
///
/// # Safety
///
/// `muxer` must be a live pointer returned by [`mediaway_ogg_muxer_create`] and not yet
/// passed to [`mediaway_ogg_muxer_close`]. `packet` must point to a valid
/// [`MediawayPacketView`] whose `payload`/`payload_len` describe a buffer valid for reads
/// for the duration of this call (or be null with length `0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_ogg_muxer_push_packet(
    muxer: *mut OggMuxerHandle,
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

/// No-op — every [`mediaway_ogg_muxer_push_packet`] call already wrote a complete,
/// independently valid Ogg page. Exposed for shape parity with the MP4/WebM muxer API.
///
/// # Safety
///
/// `muxer` must be a live pointer returned by [`mediaway_ogg_muxer_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_ogg_muxer_flush(muxer: *mut OggMuxerHandle) -> MediawayStatus {
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

/// Drain whatever muxed Ogg page bytes are ready right now into an owned buffer.
///
/// `*out_data == NULL && *out_len == 0` is a valid "nothing ready" result, not an error.
/// The returned buffer must be released with [`crate::container::mediaway_buffer_free`].
///
/// # Safety
///
/// `muxer` must be a live pointer returned by [`mediaway_ogg_muxer_create`]. `out_data` and
/// `out_len` must be valid, writable, non-null out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_ogg_muxer_poll_bytes(
    muxer: *mut OggMuxerHandle,
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

/// Close and free an Ogg muxer handle.
///
/// Always safe to call, including on a poisoned handle. In the unlikely event a panic
/// occurs while dropping the handle, the allocation is deliberately leaked rather than
/// double-handled (`adr/0001-mp4-mux-demux-c-abi.md` §7) — this would itself be a Mediaway
/// bug to fix if ever observed.
///
/// # Safety
///
/// `muxer` must be null or a pointer previously returned by [`mediaway_ogg_muxer_create`]
/// and not already passed to this function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_ogg_muxer_close(muxer: *mut OggMuxerHandle) {
    if muxer.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `muxer` is a valid, not-yet-freed handle pointer
        // (function contract).
        drop(unsafe { Box::from_raw(muxer) });
    }));
}
