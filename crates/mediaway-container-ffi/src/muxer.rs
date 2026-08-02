//! Opaque muxer handle and its C ABI functions.
//!
//! Typestate (`Open` → `Live`) and panic-safety strategy: `adr/0001-mp4-mux-demux-c-abi.md`
//! §1, §7.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mediaway_common::{Bytes, Packet, StreamInfo, VideoGeometry};
use mediaway_container::mp4;

use crate::buffer::{borrow_slice, leak_boxed_slice};
use crate::status::MediawayStatus;
use crate::types::{MediawayAudioTrackInfo, MediawayPacketView, MediawayVideoTrackInfo};

/// Opaque muxer handle (`mediaway_muxer_t*` in the C header).
///
/// Thread-confined by convention: may be moved between threads, but must not be used from
/// two threads concurrently without external synchronization.
pub struct MuxerHandle {
    poisoned: bool,
    state: MuxerState,
}

enum MuxerState {
    Open(mp4::mux::Muxer<mp4::mux::Open>),
    Live(mp4::mux::Muxer<mp4::mux::Live>),
}

/// Create a new muxer in the track-registration (`Open`) state.
///
/// Returns null only if a panic was caught during construction (defensive;
/// `mp4::mux::Muxer::new()` is simple enough that this should not trigger in practice).
#[unsafe(no_mangle)]
pub extern "C" fn mediaway_muxer_create() -> *mut MuxerHandle {
    let built = catch_unwind(AssertUnwindSafe(|| MuxerHandle {
        poisoned: false,
        state: MuxerState::Open(mp4::mux::Muxer::new()),
    }));
    built.map_or(std::ptr::null_mut(), |handle| {
        Box::into_raw(Box::new(handle))
    })
}

/// Create a new muxer in the track-registration (`Open`) state, with a custom
/// samples-per-fragment batch size instead of the core's default.
///
/// `batch == 0` is **not** rejected: it is passed straight through to
/// `mp4::mux::Muxer::with_fragment_batch`, which itself clamps it to `1`
/// (`adr/0002-clearkey-decrypt-and-fragment-batch-c-abi.md` §2). There is no diagnostic
/// for a caller that passes `0` by mistake — this mirrors the Rust core's own definition
/// of "valid", not an FFI-side error.
///
/// Returns null only if a panic was caught during construction (defensive;
/// `mp4::mux::Muxer::with_fragment_batch()` is simple enough that this should not trigger
/// in practice).
#[unsafe(no_mangle)]
pub extern "C" fn mediaway_muxer_create_with_fragment_batch(batch: usize) -> *mut MuxerHandle {
    let built = catch_unwind(AssertUnwindSafe(|| MuxerHandle {
        poisoned: false,
        state: MuxerState::Open(mp4::mux::Muxer::with_fragment_batch(batch)),
    }));
    built.map_or(std::ptr::null_mut(), |handle| {
        Box::into_raw(Box::new(handle))
    })
}

/// Register a video track on an `Open` muxer.
///
/// # Safety
///
/// `muxer` must be a live pointer returned by [`mediaway_muxer_create`] and not yet passed
/// to [`mediaway_muxer_close`]. `info` must point to a valid [`MediawayVideoTrackInfo`]
/// whose `extra_data`/`extra_data_len` describe a buffer valid for reads for the duration
/// of this call (or be null with length `0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_muxer_add_video_track(
    muxer: *mut MuxerHandle,
    info: *const MediawayVideoTrackInfo,
) -> MediawayStatus {
    if muxer.is_null() || info.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `muxer` is a valid, live handle pointer (function contract).
    let handle = unsafe { &mut *muxer };
    if handle.poisoned {
        return MediawayStatus::HandlePoisoned;
    }
    // SAFETY: caller guarantees `info` is valid for reads (function contract).
    let info = unsafe { *info };
    // SAFETY: `info.extra_data`/`info.extra_data_len` describe a buffer valid for this call
    // (function contract).
    let Some(extra_data) = (unsafe { borrow_slice(info.extra_data, info.extra_data_len) }) else {
        return MediawayStatus::InvalidArgument;
    };

    let result = catch_unwind(AssertUnwindSafe(|| {
        let MuxerState::Open(open) = &mut handle.state else {
            return Err(MediawayStatus::InvalidState);
        };
        let track = StreamInfo::Video {
            id: info.id,
            codec: info.codec.into(),
            time_base: info.time_base.into(),
            geometry: VideoGeometry {
                width: info.width,
                height: info.height,
            },
            extra_data: Bytes::copy_from_slice(extra_data),
        };
        open.add_track(track)
            .map(|_id| ())
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

/// Register an audio (or subtitle) track on an `Open` muxer.
///
/// # Safety
///
/// Same contract as [`mediaway_muxer_add_video_track`], for [`MediawayAudioTrackInfo`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_muxer_add_audio_track(
    muxer: *mut MuxerHandle,
    info: *const MediawayAudioTrackInfo,
) -> MediawayStatus {
    if muxer.is_null() || info.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `muxer` is a valid, live handle pointer (function contract).
    let handle = unsafe { &mut *muxer };
    if handle.poisoned {
        return MediawayStatus::HandlePoisoned;
    }
    // SAFETY: caller guarantees `info` is valid for reads (function contract).
    let info = unsafe { *info };
    // SAFETY: `info.extra_data`/`info.extra_data_len` describe a buffer valid for this call
    // (function contract).
    let Some(extra_data) = (unsafe { borrow_slice(info.extra_data, info.extra_data_len) }) else {
        return MediawayStatus::InvalidArgument;
    };

    let result = catch_unwind(AssertUnwindSafe(|| {
        let MuxerState::Open(open) = &mut handle.state else {
            return Err(MediawayStatus::InvalidState);
        };
        let track = StreamInfo::Audio {
            id: info.id,
            codec: info.codec.into(),
            time_base: info.time_base.into(),
            extra_data: Bytes::copy_from_slice(extra_data),
            sample_rate: info.sample_rate,
            channels: info.channels,
        };
        open.add_track(track)
            .map(|_id| ())
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

/// Transition the muxer from `Open` (track registration) to `Live` (streaming).
///
/// # Safety
///
/// `muxer` must be a live pointer returned by [`mediaway_muxer_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_muxer_begin(muxer: *mut MuxerHandle) -> MediawayStatus {
    if muxer.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `muxer` is a valid, live handle pointer (function contract).
    let handle = unsafe { &mut *muxer };
    if handle.poisoned {
        return MediawayStatus::HandlePoisoned;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        let MuxerState::Open(open) = &mut handle.state else {
            return Err(MediawayStatus::InvalidState);
        };
        let owned = std::mem::take(open);
        handle.state = MuxerState::Live(owned.begin());
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

/// Push one packet into a `Live` muxer.
///
/// # Safety
///
/// `muxer` must be a live pointer returned by [`mediaway_muxer_create`]. `packet` must
/// point to a valid [`MediawayPacketView`] whose `payload`/`payload_len` describe a buffer
/// valid for reads for the duration of this call (or be null with length `0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_muxer_push_packet(
    muxer: *mut MuxerHandle,
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

    let result = catch_unwind(AssertUnwindSafe(|| {
        let MuxerState::Live(live) = &mut handle.state else {
            return Err(MediawayStatus::InvalidState);
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
        live.push_packet(&packet).map_err(MediawayStatus::from)
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

/// Flush any pending fragments on a `Live` muxer so they become available via
/// [`mediaway_muxer_poll_bytes`].
///
/// # Safety
///
/// `muxer` must be a live pointer returned by [`mediaway_muxer_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_muxer_flush(muxer: *mut MuxerHandle) -> MediawayStatus {
    if muxer.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `muxer` is a valid, live handle pointer (function contract).
    let handle = unsafe { &mut *muxer };
    if handle.poisoned {
        return MediawayStatus::HandlePoisoned;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        let MuxerState::Live(live) = &mut handle.state else {
            return Err(MediawayStatus::InvalidState);
        };
        live.flush();
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

/// Drain whatever muxed container bytes are ready right now into an owned buffer.
///
/// `*out_data == NULL && *out_len == 0` is a valid "nothing ready" result, not an error.
/// The returned buffer must be released with [`crate::mediaway_buffer_free`].
///
/// # Safety
///
/// `muxer` must be a live pointer returned by [`mediaway_muxer_create`]. `out_data` and
/// `out_len` must be valid, writable, non-null out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_muxer_poll_bytes(
    muxer: *mut MuxerHandle,
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
        let MuxerState::Live(live) = &mut handle.state else {
            return Err(MediawayStatus::InvalidState);
        };
        let mut buf = Vec::new();
        live.poll_bytes(&mut buf);
        Ok(buf)
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

/// Close and free a muxer handle.
///
/// Always safe to call, including on a poisoned handle. In the unlikely event a panic
/// occurs while dropping the handle, the allocation is deliberately leaked rather than
/// double-handled (`adr/0001-mp4-mux-demux-c-abi.md` §7) — this would itself be a Mediaway
/// bug to fix if ever observed.
///
/// # Safety
///
/// `muxer` must be null or a pointer previously returned by [`mediaway_muxer_create`] and
/// not already passed to this function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_muxer_close(muxer: *mut MuxerHandle) {
    if muxer.is_null() {
        return;
    }
    // A panic during drop is deliberately swallowed and the allocation leaked — see the
    // doc comment above and ADR-0001 §7.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `muxer` is a valid, not-yet-freed handle pointer
        // (function contract).
        drop(unsafe { Box::from_raw(muxer) });
    }));
}
