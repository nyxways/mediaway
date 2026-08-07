//! Opaque MPEG-TS demuxer handle and its C ABI functions.
//!
//! Mirrors `mediaway_ogg_demuxer_t`/`mediaway_flv_demuxer_t` for `push_bytes`/
//! `stream_count`/`stream_at`/`poll_packet` — plus [`mediaway_ts_demuxer_finish`], a shape
//! none of the other formats need: MPEG-TS only confirms a PES packet's boundary once the
//! *next* packet on the same PID starts, so the very last access unit per PID needs an
//! explicit flush at end-of-stream. See `adr/0006-mpeg-ts-c-abi.md`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mediaway_container::ts;

use crate::container::buffer::{borrow_slice, leak_boxed_slice, reclaim_boxed_slice};
use crate::container::status::MediawayStatus;
use crate::container::types::{MediawayPacket, MediawayStreamInfo};

/// Opaque MPEG-TS demuxer handle (`mediaway_ts_demuxer_t*` in the C header).
///
/// Thread-confined by convention: may be moved between threads, but must not be used from
/// two threads concurrently without external synchronization.
pub struct TsDemuxerHandle {
    poisoned: bool,
    inner: ts::Demuxer,
}

/// Create a new, empty MPEG-TS demuxer.
///
/// Returns null only if a panic was caught during construction (defensive;
/// `ts::Demuxer::new()` is simple enough that this should not trigger in practice).
#[unsafe(no_mangle)]
pub extern "C" fn mediaway_ts_demuxer_create() -> *mut TsDemuxerHandle {
    let built = catch_unwind(AssertUnwindSafe(|| TsDemuxerHandle {
        poisoned: false,
        inner: ts::Demuxer::new(),
    }));
    built.map_or(std::ptr::null_mut(), |handle| {
        Box::into_raw(Box::new(handle))
    })
}

/// Feed bytes (need not be 188-byte aligned across calls).
///
/// # Safety
///
/// `demuxer` must be a live pointer returned by [`mediaway_ts_demuxer_create`]. `data` must
/// be valid for reads of `len` bytes for the duration of this call (or null with
/// `len == 0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_ts_demuxer_push_bytes(
    demuxer: *mut TsDemuxerHandle,
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

/// Number of streams whose `stream_type` maps to a codec this facade recognizes.
///
/// H.264/HEVC/AAC/MP3 only. Empty until [`mediaway_ts_demuxer_poll_packet`] has actually
/// consumed the PMT packet — this crate parses PAT/PMT lazily (see
/// `mediaway-container::ts` module docs).
///
/// Takes a `const` handle: on a caught panic (not expected in practice, since this only
/// reads a slice length) this returns `0` without poisoning the handle, since there is no
/// mutable access here to record the flag.
///
/// # Safety
///
/// `demuxer` must be a live pointer returned by [`mediaway_ts_demuxer_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_ts_demuxer_stream_count(
    demuxer: *const TsDemuxerHandle,
) -> usize {
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

/// Get stream info by index. `id` is the TS PID.
///
/// # Safety
///
/// `demuxer` must be a live pointer returned by [`mediaway_ts_demuxer_create`]. `out_info`
/// must be a valid, writable pointer to a [`MediawayStreamInfo`]; on success it must later
/// be released with [`crate::container::mediaway_stream_info_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_ts_demuxer_stream_at(
    demuxer: *mut TsDemuxerHandle,
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

/// Pop the next demuxed packet, if any is ready. A PID with no recognized codec mapping is
/// silently skipped (see [`mediaway_ts_demuxer_stream_count`]'s docs).
///
/// `*out_has_packet == false` is a valid "nothing ready" result, not an error;
/// `*out_packet` is only meaningful when `*out_has_packet == true`, and must then be
/// released with [`crate::container::mediaway_packet_free`].
///
/// # Safety
///
/// `demuxer` must be a live pointer returned by [`mediaway_ts_demuxer_create`].
/// `out_packet` must be a valid, writable pointer to a [`MediawayPacket`]. `out_has_packet`
/// must be a valid, writable `bool` pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_ts_demuxer_poll_packet(
    demuxer: *mut TsDemuxerHandle,
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
        handle.inner.poll_packet().map(|p| to_ffi_packet(&p))
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

/// Force-emit whatever is still accumulating per PID — call once at the end of a stream so
/// the very last access unit per PID isn't lost (see module docs).
///
/// `*out_packets`/`*out_count` describe an owned array (possibly empty: `*out_count == 0`
/// with `*out_packets == NULL` is valid, not an error), released with
/// [`mediaway_ts_demuxer_finish_free`] — **not** [`crate::container::mediaway_packet_free`],
/// which only knows how to free one packet, not an array.
///
/// # Safety
///
/// `demuxer` must be a live pointer returned by [`mediaway_ts_demuxer_create`].
/// `out_packets` and `out_count` must be valid, writable, non-null out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_ts_demuxer_finish(
    demuxer: *mut TsDemuxerHandle,
    out_packets: *mut *mut MediawayPacket,
    out_count: *mut usize,
) -> MediawayStatus {
    if demuxer.is_null() || out_packets.is_null() || out_count.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `demuxer` is a valid, live handle pointer (function
    // contract).
    let handle = unsafe { &mut *demuxer };
    if handle.poisoned {
        return MediawayStatus::HandlePoisoned;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        handle
            .inner
            .finish()
            .iter()
            .map(to_ffi_packet)
            .collect::<Vec<_>>()
    }));

    if let Ok(packets) = result {
        let (ptr, len) = leak_packet_array(packets);
        // SAFETY: `out_packets`/`out_count` are checked non-null above (function contract).
        unsafe {
            out_packets.write(ptr);
            out_count.write(len);
        }
        MediawayStatus::Ok
    } else {
        handle.poisoned = true;
        MediawayStatus::InternalPanic
    }
}

/// Free an array returned by [`mediaway_ts_demuxer_finish`].
///
/// # Safety
///
/// `packets`/`count` must be exactly the pointer/length pair returned by that function (or
/// `(null, 0)`), and must not have already been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_ts_demuxer_finish_free(
    packets: *mut MediawayPacket,
    count: usize,
) {
    if packets.is_null() || count == 0 {
        return;
    }
    // SAFETY: caller guarantees `packets`/`count` came from `mediaway_ts_demuxer_finish` and
    // are not yet freed (function contract).
    let boxed = unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(packets, count)) };
    for packet in &boxed {
        // SAFETY: each packet's `payload`/`payload_len` were produced by `leak_boxed_slice`
        // inside `to_ffi_packet` (function contract).
        unsafe { reclaim_boxed_slice(packet.payload, packet.payload_len) };
    }
}

fn to_ffi_packet(packet: &mediaway_common::Packet) -> MediawayPacket {
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
}

fn leak_packet_array(data: Vec<MediawayPacket>) -> (*mut MediawayPacket, usize) {
    if data.is_empty() {
        return (std::ptr::null_mut(), 0);
    }
    let boxed = data.into_boxed_slice();
    let len = boxed.len();
    (Box::into_raw(boxed).cast::<MediawayPacket>(), len)
}

/// Close and free an MPEG-TS demuxer handle.
///
/// Always safe to call, including on a poisoned handle. In the unlikely event a panic
/// occurs while dropping the handle, the allocation is deliberately leaked rather than
/// double-handled (`adr/0001-mp4-mux-demux-c-abi.md` §7) — this would itself be a Mediaway
/// bug to fix if ever observed.
///
/// # Safety
///
/// `demuxer` must be null or a pointer previously returned by
/// [`mediaway_ts_demuxer_create`] and not already passed to this function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_ts_demuxer_close(demuxer: *mut TsDemuxerHandle) {
    if demuxer.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `demuxer` is a valid, not-yet-freed handle pointer
        // (function contract).
        drop(unsafe { Box::from_raw(demuxer) });
    }));
}
