//! Buffer ownership helpers and the shared `_free` functions.
//!
//! Ownership rules: `adr/0001-mp4-mux-demux-c-abi.md` §6. `borrow_slice`/
//! `leak_boxed_slice`/`reclaim_boxed_slice` are implemented in `mediaway-common-ffi`
//! and imported here (`docs/adr/0015-common-ffi-unification.md`) — this crate's own
//! exported `_free` function names/signatures are unchanged.

// Re-exported (not just `use`d) so sibling modules can keep referring to
// `crate::buffer::{borrow_slice, leak_boxed_slice}` unchanged (muxer.rs/demuxer.rs).
// `pub(crate)`, not `pub`, because they are internal-only (never re-exported from
// `lib.rs`) — that is genuinely the intended visibility, not redundancy.
// `clippy::redundant_pub_crate` disagrees with rustc's own `unreachable_pub` lint for
// this exact "pub(crate) item in a private module" shape; rustc's lint wins here.
#![allow(clippy::redundant_pub_crate)]

#[cfg(feature = "demux")]
use crate::types::{MediawayPacket, MediawayStreamInfo};
pub(crate) use mediaway_common_ffi::buffer::{borrow_slice, leak_boxed_slice, reclaim_boxed_slice};

/// Free a buffer returned by [`crate::mediaway_muxer_poll_bytes`].
///
/// # Safety
///
/// `data`/`len` must be exactly the pointer/length pair returned by that function (or
/// `(null, 0)`), and must not have already been freed. Thread-confined: do not call
/// concurrently with another call passing the same `data`.
#[cfg(feature = "mux")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_buffer_free(data: *mut u8, len: usize) {
    // SAFETY: caller guarantees `data`/`len` came from `mediaway_muxer_poll_bytes` and are
    // not yet freed (function contract).
    unsafe { reclaim_boxed_slice(data, len) };
}

/// Free a packet returned by [`crate::mediaway_demuxer_poll_packet`].
///
/// Nulls the packet's payload pointer/length afterward, making a double-free a visible
/// no-op instead of undefined behavior.
///
/// # Safety
///
/// `packet` must be null or a valid, writable pointer to a [`MediawayPacket`] whose
/// `payload`/`payload_len` were produced by that function and not already freed.
#[cfg(feature = "demux")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_packet_free(packet: *mut MediawayPacket) {
    if packet.is_null() {
        return;
    }
    // SAFETY: caller guarantees `packet` is a valid, writable pointer (function contract).
    let packet = unsafe { &mut *packet };
    // SAFETY: `packet.payload`/`packet.payload_len` were produced by `leak_boxed_slice` via
    // `mediaway_demuxer_poll_packet` (function contract).
    unsafe { reclaim_boxed_slice(packet.payload, packet.payload_len) };
    packet.payload = std::ptr::null_mut();
    packet.payload_len = 0;
}

/// Free stream info returned by [`crate::mediaway_demuxer_stream_at`].
///
/// Nulls the struct's `extra_data` pointer/length afterward, making a double-free a
/// visible no-op instead of undefined behavior.
///
/// # Safety
///
/// `info` must be null or a valid, writable pointer to a [`MediawayStreamInfo`] whose
/// `extra_data`/`extra_data_len` were produced by that function and not already freed.
#[cfg(feature = "demux")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_stream_info_free(info: *mut MediawayStreamInfo) {
    if info.is_null() {
        return;
    }
    // SAFETY: caller guarantees `info` is a valid, writable pointer (function contract).
    let info = unsafe { &mut *info };
    // SAFETY: `info.extra_data`/`info.extra_data_len` were produced by `leak_boxed_slice` via
    // `mediaway_demuxer_stream_at` (function contract).
    unsafe { reclaim_boxed_slice(info.extra_data, info.extra_data_len) };
    info.extra_data = std::ptr::null_mut();
    info.extra_data_len = 0;
}
