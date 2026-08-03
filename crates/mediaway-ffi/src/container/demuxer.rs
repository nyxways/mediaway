//! Opaque demuxer handle and its C ABI functions.
//!
//! Panic-safety strategy: `adr/0001-mp4-mux-demux-c-abi.md` §7.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mediaway_container::mp4;

use crate::container::buffer::{borrow_slice, leak_boxed_slice};
use crate::container::status::MediawayStatus;
use crate::container::types::{MediawayPacket, MediawayStreamInfo};

/// Opaque demuxer handle (`mediaway_demuxer_t*` in the C header).
///
/// Thread-confined by convention: may be moved between threads, but must not be used from
/// two threads concurrently without external synchronization.
pub struct DemuxerHandle {
    poisoned: bool,
    inner: mp4::Demuxer,
}

/// Create a new, empty demuxer.
///
/// Returns null only if a panic was caught during construction (defensive;
/// `mp4::Demuxer::new()` is simple enough that this should not trigger in practice).
#[unsafe(no_mangle)]
pub extern "C" fn mediaway_demuxer_create() -> *mut DemuxerHandle {
    let built = catch_unwind(AssertUnwindSafe(|| DemuxerHandle {
        poisoned: false,
        inner: mp4::Demuxer::new(),
    }));
    built.map_or(std::ptr::null_mut(), |handle| {
        Box::into_raw(Box::new(handle))
    })
}

/// Feed container bytes into the demuxer. The core copies the bytes synchronously before
/// returning; the FFI layer adds no extra copy beyond that (`adr/0001-mp4-mux-demux-c-abi.md`
/// §6).
///
/// # Safety
///
/// `demuxer` must be a live pointer returned by [`mediaway_demuxer_create`]. `data` must be
/// valid for reads of `len` bytes for the duration of this call (or null with `len == 0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_demuxer_push_bytes(
    demuxer: *mut DemuxerHandle,
    data: *const u8,
    len: usize,
) -> MediawayStatus {
    if demuxer.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `demuxer` is a valid, live handle pointer (function
    // contract).
    let handle = unsafe { &mut *demuxer };
    if handle.poisoned {
        return MediawayStatus::HandlePoisoned;
    }
    // SAFETY: `data`/`len` describe a buffer valid for this call (function contract).
    let Some(chunk) = (unsafe { borrow_slice(data, len) }) else {
        return MediawayStatus::InvalidArgument;
    };

    let result = catch_unwind(AssertUnwindSafe(|| {
        handle.inner.push_bytes(chunk);
    }));

    if result.is_ok() {
        MediawayStatus::Ok
    } else {
        handle.poisoned = true;
        MediawayStatus::InternalPanic
    }
}

/// Number of streams/tracks discovered so far.
///
/// Takes a `const` handle: on a caught panic (not expected in practice, since this only
/// reads a `Vec` length) this returns `0` without poisoning the handle, since there is no
/// mutable access here to record the flag.
///
/// # Safety
///
/// `demuxer` must be a live pointer returned by [`mediaway_demuxer_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_demuxer_stream_count(demuxer: *const DemuxerHandle) -> usize {
    if demuxer.is_null() {
        return 0;
    }
    // SAFETY: caller guarantees `demuxer` is a valid, live handle pointer (function
    // contract).
    let handle = unsafe { &*demuxer };
    if handle.poisoned {
        return 0;
    }
    catch_unwind(AssertUnwindSafe(|| handle.inner.streams().len())).unwrap_or(0)
}

/// Get stream/track info by index.
///
/// # Safety
///
/// `demuxer` must be a live pointer returned by [`mediaway_demuxer_create`]. `out_info`
/// must be a valid, writable pointer to a [`MediawayStreamInfo`]; on success it must later
/// be released with [`crate::container::mediaway_stream_info_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_demuxer_stream_at(
    demuxer: *mut DemuxerHandle,
    index: usize,
    out_info: *mut MediawayStreamInfo,
) -> MediawayStatus {
    if demuxer.is_null() || out_info.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `demuxer` is a valid, live handle pointer (function
    // contract).
    let handle = unsafe { &mut *demuxer };
    if handle.poisoned {
        return MediawayStatus::HandlePoisoned;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        let Some(stream) = handle.inner.streams().get(index) else {
            return Err(MediawayStatus::InvalidArgument);
        };
        let geometry = stream.geometry();
        let (extra_data, extra_data_len) = leak_boxed_slice(stream.extra_data().to_vec());
        Ok(MediawayStreamInfo {
            id: stream.id(),
            codec: stream.codec().into(),
            time_base: stream.time_base().into(),
            has_geometry: geometry.is_some(),
            width: geometry.map_or(0, |g| g.width),
            height: geometry.map_or(0, |g| g.height),
            sample_rate: stream.sample_rate().unwrap_or(0),
            channels: stream.channels().unwrap_or(0),
            extra_data,
            extra_data_len,
        })
    }));

    match result {
        Ok(Ok(info)) => {
            // SAFETY: `out_info` is checked non-null above (function contract).
            unsafe { out_info.write(info) };
            MediawayStatus::Ok
        }
        Ok(Err(status)) => status,
        Err(_) => {
            handle.poisoned = true;
            MediawayStatus::InternalPanic
        }
    }
}

/// Pop the next demuxed packet, if any is ready.
///
/// `*out_has_packet == false` is a valid "nothing ready" result, not an error; `*out_packet`
/// is only meaningful when `*out_has_packet == true`, and must then be released with
/// [`crate::container::mediaway_packet_free`].
///
/// # Safety
///
/// `demuxer` must be a live pointer returned by [`mediaway_demuxer_create`]. `out_packet`
/// must be a valid, writable pointer to a [`MediawayPacket`]. `out_has_packet` must be a
/// valid, writable `bool` pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_demuxer_poll_packet(
    demuxer: *mut DemuxerHandle,
    out_packet: *mut MediawayPacket,
    out_has_packet: *mut bool,
) -> MediawayStatus {
    if demuxer.is_null() || out_packet.is_null() || out_has_packet.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `demuxer` is a valid, live handle pointer (function
    // contract).
    let handle = unsafe { &mut *demuxer };
    if handle.poisoned {
        return MediawayStatus::HandlePoisoned;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        handle.inner.poll_packet().map(|packet| {
            let (payload, payload_len) = leak_boxed_slice(packet.payload.to_vec());
            MediawayPacket {
                stream_id: packet.stream_id,
                pts: packet.pts,
                dts: packet.dts,
                duration: packet.duration,
                is_keyframe: packet.is_keyframe,
                is_discard: packet.is_discard,
                payload,
                payload_len,
            }
        })
    }));

    match result {
        Ok(Some(packet)) => {
            // SAFETY: `out_packet`/`out_has_packet` are checked non-null above (function
            // contract).
            unsafe {
                out_packet.write(packet);
                out_has_packet.write(true);
            }
            MediawayStatus::Ok
        }
        Ok(None) => {
            // SAFETY: `out_has_packet` is checked non-null above (function contract).
            unsafe { out_has_packet.write(false) };
            MediawayStatus::Ok
        }
        Err(_) => {
            handle.poisoned = true;
            MediawayStatus::InternalPanic
        }
    }
}

/// Set the `ClearKey` decryption key applied to all encrypted tracks on this demuxer.
///
/// One demuxer-wide key, no track/KID scoping (`adr/0002-clearkey-decrypt-and-fragment-batch-c-abi.md`
/// §1): `iso_bmff::Demuxer` never compares this key against a track's parsed KID, so
/// multi-KID content decrypted with a key that is wrong for even one track — or with no
/// key set at all — silently yields garbage or raw-ciphertext sample payloads marked as
/// ordinary (non-discarded) packets, not an error.
///
/// **Timing contract:** decrypt runs synchronously inside [`mediaway_demuxer_push_bytes`].
/// This call only affects samples drained from *subsequent* `push_bytes` calls; it does
/// not retroactively re-decrypt packets already sitting in the poll queue.
///
/// # Safety
///
/// `demuxer` must be a live pointer returned by [`mediaway_demuxer_create`]. `key` must be
/// valid for reads of `key_len` bytes for the duration of this call (or null with
/// `key_len == 0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_demuxer_set_decryption_key(
    demuxer: *mut DemuxerHandle,
    key: *const u8,
    key_len: usize,
) -> MediawayStatus {
    if demuxer.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `demuxer` is a valid, live handle pointer (function
    // contract).
    let handle = unsafe { &mut *demuxer };
    if handle.poisoned {
        return MediawayStatus::HandlePoisoned;
    }
    // SAFETY: `key`/`key_len` describe a buffer valid for this call (function contract).
    let Some(key_slice) = (unsafe { borrow_slice(key, key_len) }) else {
        return MediawayStatus::InvalidArgument;
    };
    let Ok(key_arr) = <[u8; 16]>::try_from(key_slice) else {
        return MediawayStatus::InvalidArgument;
    };

    let result = catch_unwind(AssertUnwindSafe(|| {
        handle.inner.set_decryption_key(key_arr);
    }));

    if result.is_ok() {
        MediawayStatus::Ok
    } else {
        handle.poisoned = true;
        MediawayStatus::InternalPanic
    }
}

/// Clear a previously set `ClearKey` decryption key, so subsequent encrypted samples are no
/// longer decrypted.
///
/// Same timing contract as [`mediaway_demuxer_set_decryption_key`]: only affects samples
/// drained from *subsequent* `push_bytes` calls.
///
/// # Safety
///
/// `demuxer` must be a live pointer returned by [`mediaway_demuxer_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_demuxer_clear_decryption_key(
    demuxer: *mut DemuxerHandle,
) -> MediawayStatus {
    if demuxer.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `demuxer` is a valid, live handle pointer (function
    // contract).
    let handle = unsafe { &mut *demuxer };
    if handle.poisoned {
        return MediawayStatus::HandlePoisoned;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        handle.inner.clear_decryption_key();
    }));

    if result.is_ok() {
        MediawayStatus::Ok
    } else {
        handle.poisoned = true;
        MediawayStatus::InternalPanic
    }
}

/// Close and free a demuxer handle.
///
/// Always safe to call, including on a poisoned handle. In the unlikely event a panic
/// occurs while dropping the handle, the allocation is deliberately leaked rather than
/// double-handled (`adr/0001-mp4-mux-demux-c-abi.md` §7) — this would itself be a Mediaway
/// bug to fix if ever observed.
///
/// # Safety
///
/// `demuxer` must be null or a pointer previously returned by [`mediaway_demuxer_create`]
/// and not already passed to this function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_demuxer_close(demuxer: *mut DemuxerHandle) {
    if demuxer.is_null() {
        return;
    }
    // A panic during drop is deliberately swallowed and the allocation leaked — see the
    // doc comment above and ADR-0001 §7.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `demuxer` is a valid, not-yet-freed handle pointer
        // (function contract).
        drop(unsafe { Box::from_raw(demuxer) });
    }));
}
