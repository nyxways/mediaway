//! Opaque Ogg demuxer handle and its C ABI functions.
//!
//! Dedicated shape, not a `DemuxerState` variant: reachable through the shared
//! `mediaway_demuxer_t` only requires implementing [`mediaway_container::Demux`], which
//! `ogg::Demuxer` does — but it is kept as its own handle type anyway, mirroring the muxer
//! side's dedicated `mediaway_ogg_muxer_t`, so a caller who only wants Ogg is not exposed to
//! MP4/WebM-only functions (`*_set_decryption_key`) that would always fail on it. See
//! `adr/0004-ogg-adts-c-abi.md`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mediaway_container::ogg;

use crate::container::buffer::{borrow_slice, leak_boxed_slice};
use crate::container::status::MediawayStatus;
use crate::container::types::{MediawayPacket, MediawayStreamInfo};

/// Opaque Ogg demuxer handle (`mediaway_ogg_demuxer_t*` in the C header).
///
/// Thread-confined by convention: may be moved between threads, but must not be used from
/// two threads concurrently without external synchronization.
pub struct OggDemuxerHandle {
    poisoned: bool,
    inner: ogg::Demuxer,
}

/// Create a new, empty Ogg demuxer.
///
/// Returns null only if a panic was caught during construction (defensive;
/// `ogg::Demuxer::new()` is simple enough that this should not trigger in practice).
#[unsafe(no_mangle)]
pub extern "C" fn mediaway_ogg_demuxer_create() -> *mut OggDemuxerHandle {
    let built = catch_unwind(AssertUnwindSafe(|| OggDemuxerHandle {
        poisoned: false,
        inner: ogg::Demuxer::new(),
    }));
    built.map_or(std::ptr::null_mut(), |handle| {
        Box::into_raw(Box::new(handle))
    })
}

/// Feed Ogg-container bytes into the demuxer.
///
/// # Safety
///
/// `demuxer` must be a live pointer returned by [`mediaway_ogg_demuxer_create`]. `data` must
/// be valid for reads of `len` bytes for the duration of this call (or null with `len == 0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_ogg_demuxer_push_bytes(
    demuxer: *mut OggDemuxerHandle,
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

/// Number of logical bitstreams discovered so far — `0` or `1` (Ogg carries a single
/// logical bitstream per this facade; see `mediaway-container::ogg` module docs).
///
/// Takes a `const` handle: on a caught panic (not expected in practice, since this only
/// reads a slice length) this returns `0` without poisoning the handle, since there is no
/// mutable access here to record the flag.
///
/// # Safety
///
/// `demuxer` must be a live pointer returned by [`mediaway_ogg_demuxer_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_ogg_demuxer_stream_count(
    demuxer: *const OggDemuxerHandle,
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

/// Get stream info by index (always index `0` once the identification-header packet has
/// been recognized).
///
/// # Safety
///
/// `demuxer` must be a live pointer returned by [`mediaway_ogg_demuxer_create`]. `out_info`
/// must be a valid, writable pointer to a [`MediawayStreamInfo`]; on success it must later
/// be released with [`crate::container::mediaway_stream_info_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_ogg_demuxer_stream_at(
    demuxer: *mut OggDemuxerHandle,
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
/// `demuxer` must be a live pointer returned by [`mediaway_ogg_demuxer_create`].
/// `out_packet` must be a valid, writable pointer to a [`MediawayPacket`]. `out_has_packet`
/// must be a valid, writable `bool` pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_ogg_demuxer_poll_packet(
    demuxer: *mut OggDemuxerHandle,
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

/// Close and free an Ogg demuxer handle.
///
/// Always safe to call, including on a poisoned handle. In the unlikely event a panic
/// occurs while dropping the handle, the allocation is deliberately leaked rather than
/// double-handled (`adr/0001-mp4-mux-demux-c-abi.md` §7) — this would itself be a Mediaway
/// bug to fix if ever observed.
///
/// # Safety
///
/// `demuxer` must be null or a pointer previously returned by
/// [`mediaway_ogg_demuxer_create`] and not already passed to this function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_ogg_demuxer_close(demuxer: *mut OggDemuxerHandle) {
    if demuxer.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `demuxer` is a valid, not-yet-freed handle pointer
        // (function contract).
        drop(unsafe { Box::from_raw(demuxer) });
    }));
}
