//! Replay ring C ABI — `mediaway_container::replay::ReplayRing` reachable from C.
//!
//! Design and reasons: `adr/container/0009-replay-ring-c-abi.md`. In short: one opaque ring
//! handle over a closed `enum` of the two payload kinds (`Bytes`, `StoredPayload`), no
//! `Box<dyn>`; a clip is an **owned snapshot**, not the borrowing `Clip` the Rust API returns,
//! so a C caller can keep pushing while it reads one; and there is deliberately no "mux this
//! clip" function — the clip's packets go through the existing muxer (`add_track` +
//! `push_packet`), the same composition the Rust API has.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use mediaway_common::{Bytes, Packet};
use mediaway_container::replay::{PacketMeta, ReplayError, ReplayRing, StoredPayload};

use crate::container::buffer::borrow_slice;
use crate::container::status::MediawayStatus;
use crate::container::types::{MediawayPacketView, MediawayRational};

/// What a ring holds for each packet. Zero is [`Self::Bytes`].
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediawayReplayPayloadKind {
    /// The payload bytes, copied in by `mediaway_replay_ring_push` and shared with every clip.
    Bytes = 0,
    /// Only where the bytes are (`mediaway_replay_ring_push_stored`): the caller keeps them in a
    /// file it already writes, and reads them back itself when it saves a clip.
    Stored = 1,
}

/// Config for [`mediaway_replay_ring_create`] — plain value, no free function.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MediawayReplayRingConfig {
    /// The stream whose keyframes decide where a clip may start (normally video).
    pub anchor_stream_id: u32,
    /// The anchor's timebase.
    pub anchor_time_base: MediawayRational,
    /// How much history to keep, in milliseconds.
    pub window_ms: u64,
    /// Evict oldest-GOP-first while more than this many payload bytes are held; `0` = no
    /// ceiling. The newest GOP is always kept, so one larger GOP can exceed it.
    pub max_bytes: u64,
    /// What each packet's payload is held as.
    pub payload_kind: MediawayReplayPayloadKind,
}

/// A packet's metadata, without its payload — input to
/// [`mediaway_replay_ring_push_stored`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MediawayPacketMeta {
    /// Stream / track id.
    pub stream_id: u32,
    /// Presentation timestamp in the stream's timebase.
    pub pts: i64,
    /// Decode timestamp in the stream's timebase. Must not go backwards within a stream.
    pub dts: i64,
    /// Duration in the stream's timebase.
    pub duration: u64,
    /// Whether this is a keyframe / random access point.
    pub is_keyframe: bool,
    /// Whether this packet is outside the active edit window.
    pub is_discard: bool,
}

/// Where a packet's payload was stored on the caller's disk.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MediawayStoredPayload {
    /// The caller's own id for the file the bytes are in.
    pub file: u32,
    /// Byte offset of the payload in that file.
    pub offset: u64,
    /// Payload length in bytes.
    pub len: u32,
}

/// One packet of a clip, from [`mediaway_replay_clip_packet_at`].
///
/// `pts` and `dts` are rebased so the cut keyframe decodes at zero. For a `Bytes` ring,
/// `payload`/`payload_len` are **borrowed from the clip** and valid until
/// [`mediaway_replay_clip_free`]. For a `Stored` ring they are `NULL`/`0` and the `stored_*`
/// fields say where the bytes are.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MediawayReplayClipEntry {
    /// Stream / track id.
    pub stream_id: u32,
    /// Presentation timestamp, rebased.
    pub pts: i64,
    /// Decode timestamp, rebased.
    pub dts: i64,
    /// Duration in the stream's timebase.
    pub duration: u64,
    /// Whether this is a keyframe.
    pub is_keyframe: bool,
    /// Whether this packet is outside the active edit window.
    pub is_discard: bool,
    /// Which of the payload fields is valid.
    pub payload_kind: MediawayReplayPayloadKind,
    /// `Bytes` only: borrowed payload, valid until the clip is freed.
    pub payload: *const u8,
    /// `Bytes` only: length of `payload`.
    pub payload_len: usize,
    /// `Stored` only: the caller's file id.
    pub stored_file: u32,
    /// `Stored` only: byte offset in that file.
    pub stored_offset: u64,
    /// `Stored` only: payload length in bytes.
    pub stored_len: u32,
}

/// What a ring holds: a closed set, so an enum rather than a trait object.
enum RingInner {
    Bytes(ReplayRing<Bytes>),
    Stored(ReplayRing<StoredPayload>),
}

/// Opaque replay-ring handle (`mediaway_replay_ring_t*` in the C header).
///
/// Thread-confined by convention: may be moved between threads, but must not be used from two
/// threads concurrently without external synchronization.
pub struct ReplayRingHandle {
    poisoned: bool,
    inner: RingInner,
}

/// One clip packet: its rebased metadata and payload.
enum ClipPayload {
    Bytes(Bytes),
    Stored(StoredPayload),
}

/// Opaque clip handle (`mediaway_replay_clip_t*` in the C header): an owned snapshot.
///
/// Unlike the Rust `Clip`, which borrows the ring and must be written out before the next
/// push, this holds its own copy of the metadata and refcounted `Bytes` (not the payload
/// bytes), so the ring can keep taking packets while a clip is being read.
pub struct ReplayClipHandle {
    entries: Vec<(PacketMeta, ClipPayload)>,
    duration: Duration,
}

/// The ring's error, as the shared container status. No new status values: an unknown stream is
/// `UnknownStream`, and a `dts` that went backwards is `InvalidPacket` (the caller can drop that
/// packet and carry on).
const fn status_of(err: &ReplayError) -> MediawayStatus {
    match err {
        ReplayError::UnknownStream { .. } => MediawayStatus::UnknownStream,
        ReplayError::DuplicateStream { .. } => MediawayStatus::InvalidTrack,
        ReplayError::BadTimebase { .. } => MediawayStatus::InvalidArgument,
        ReplayError::OutOfOrder { .. } => MediawayStatus::InvalidPacket,
        _ => MediawayStatus::UnknownError,
    }
}

fn to_millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// Create a replay ring.
///
/// Three outcomes: `Ok` writes the handle to `*out_ring`; a normal error leaves `*out_ring`
/// `NULL` (a zero timebase numerator or denominator is `INVALID_ARGUMENT`); a caught panic is
/// `INTERNAL_PANIC` with `*out_ring` `NULL`.
///
/// # Safety
///
/// `config` must be a valid, readable [`MediawayReplayRingConfig`] pointer. `out_ring` must be a
/// valid, writable, non-null out-parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_replay_ring_create(
    config: *const MediawayReplayRingConfig,
    out_ring: *mut *mut ReplayRingHandle,
) -> MediawayStatus {
    if config.is_null() || out_ring.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `config` is valid for reads (function contract).
    let config = unsafe { *config };
    // SAFETY: `out_ring` is checked non-null above and writable per the function contract.
    unsafe { out_ring.write(std::ptr::null_mut()) };

    let result = catch_unwind(AssertUnwindSafe(|| {
        let window = Duration::from_millis(config.window_ms);
        let anchor = config.anchor_stream_id;
        let timebase = config.anchor_time_base.into();
        // 0 means no ceiling; a ceiling above `usize::MAX` on a 32-bit target is no ceiling.
        let ceiling = usize::try_from(config.max_bytes).ok().filter(|&n| n > 0);
        match config.payload_kind {
            MediawayReplayPayloadKind::Bytes => {
                ReplayRing::<Bytes>::for_payload(anchor, timebase, window).map(|ring| {
                    RingInner::Bytes(match ceiling {
                        Some(n) => ring.with_max_bytes(n),
                        None => ring,
                    })
                })
            }
            MediawayReplayPayloadKind::Stored => {
                ReplayRing::<StoredPayload>::for_payload(anchor, timebase, window).map(|ring| {
                    RingInner::Stored(match ceiling {
                        Some(n) => ring.with_max_bytes(n),
                        None => ring,
                    })
                })
            }
        }
    }));

    match result {
        Ok(Ok(inner)) => {
            let handle = Box::new(ReplayRingHandle {
                poisoned: false,
                inner,
            });
            // SAFETY: `out_ring` is checked non-null above (function contract).
            unsafe { out_ring.write(Box::into_raw(handle)) };
            MediawayStatus::Ok
        }
        Ok(Err(err)) => status_of(&err),
        Err(_) => MediawayStatus::InternalPanic,
    }
}

/// Runs `op` on the ring behind `ring`, mapping a caught panic to `INTERNAL_PANIC` and poisoning
/// the handle, and a ring error to its status.
///
/// # Safety
///
/// `ring` must be a valid, live handle pointer (each caller's function contract).
unsafe fn with_ring<T>(
    ring: *mut ReplayRingHandle,
    op: impl FnOnce(&mut RingInner) -> Result<T, MediawayStatus>,
) -> Result<T, MediawayStatus> {
    if ring.is_null() {
        return Err(MediawayStatus::InvalidArgument);
    }
    // SAFETY: caller guarantees `ring` is a valid, live handle pointer (function contract).
    let handle = unsafe { &mut *ring };
    if handle.poisoned {
        return Err(MediawayStatus::HandlePoisoned);
    }
    catch_unwind(AssertUnwindSafe(|| op(&mut handle.inner))).unwrap_or_else(|_| {
        handle.poisoned = true;
        Err(MediawayStatus::InternalPanic)
    })
}

const fn ok_or_status(result: Result<(), MediawayStatus>) -> MediawayStatus {
    match result {
        Ok(()) => MediawayStatus::Ok,
        Err(status) => status,
    }
}

/// Carry another stream (e.g. audio), cut by time to match the anchor.
///
/// # Safety
///
/// `ring` must be a live pointer returned by [`mediaway_replay_ring_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_replay_ring_add_stream(
    ring: *mut ReplayRingHandle,
    stream_id: u32,
    time_base: MediawayRational,
) -> MediawayStatus {
    // SAFETY: forwarded function contract.
    let result = unsafe {
        with_ring(ring, |inner| {
            let tb = time_base.into();
            match inner {
                RingInner::Bytes(r) => r.add_stream(stream_id, tb),
                RingInner::Stored(r) => r.add_stream(stream_id, tb),
            }
            .map_err(|e| status_of(&e))
        })
    };
    ok_or_status(result)
}

/// Add a packet to a `BYTES` ring, then evict whatever fell out of the window.
///
/// The payload is **copied** into the ring (the view is borrowed for the call only, so one copy
/// is unavoidable here); every clip then shares that copy by reference count. Anchor packets
/// before the anchor's first keyframe are dropped without error: nothing can start from them.
///
/// `UNKNOWN_STREAM` for a stream never added, `INVALID_PACKET` when this stream's `dts` went
/// backwards (the packet is not added; carry on), and `INVALID_STATE` on a `STORED` ring.
///
/// # Safety
///
/// `ring` must be a live pointer returned by [`mediaway_replay_ring_create`]. `packet` must be a
/// valid, readable pointer whose `payload` (when `payload_len > 0`) points to `payload_len`
/// readable bytes, both valid for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_replay_ring_push(
    ring: *mut ReplayRingHandle,
    packet: *const MediawayPacketView,
) -> MediawayStatus {
    if packet.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `packet` is valid for reads (function contract).
    let view = unsafe { *packet };
    // SAFETY: `payload`/`payload_len` describe a buffer valid for this call (function contract).
    let Some(payload) = (unsafe { borrow_slice(view.payload, view.payload_len) }) else {
        return MediawayStatus::InvalidArgument;
    };
    // SAFETY: forwarded function contract.
    let result = unsafe {
        with_ring(ring, |inner| match inner {
            RingInner::Bytes(r) => r
                .push(Packet {
                    stream_id: view.stream_id,
                    pts: view.pts,
                    dts: view.dts,
                    duration: view.duration,
                    is_keyframe: view.is_keyframe,
                    is_discard: view.is_discard,
                    payload: Bytes::copy_from_slice(payload),
                })
                .map_err(|e| status_of(&e)),
            RingInner::Stored(_) => Err(MediawayStatus::InvalidState),
        })
    };
    ok_or_status(result)
}

/// Add a packet to a `STORED` ring: its metadata, and where its bytes are.
///
/// The ring never reads the file; it holds a few dozen bytes per packet. Take `file`, `offset`
/// and `len` from `mediaway_muxer_poll_placements` when the caller writes every polled muxer
/// byte to a file. Same statuses as [`mediaway_replay_ring_push`], with `INVALID_STATE` on a
/// `BYTES` ring.
///
/// # Safety
///
/// `ring` must be a live pointer returned by [`mediaway_replay_ring_create`]. `meta` and
/// `stored` must be valid, readable pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_replay_ring_push_stored(
    ring: *mut ReplayRingHandle,
    meta: *const MediawayPacketMeta,
    stored: *const MediawayStoredPayload,
) -> MediawayStatus {
    if meta.is_null() || stored.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees both are valid for reads (function contract).
    let (meta, stored) = unsafe { (*meta, *stored) };
    // SAFETY: forwarded function contract.
    let result = unsafe {
        with_ring(ring, |inner| match inner {
            RingInner::Stored(r) => r
                .push_entry(
                    PacketMeta {
                        stream_id: meta.stream_id,
                        pts: meta.pts,
                        dts: meta.dts,
                        duration: meta.duration,
                        is_keyframe: meta.is_keyframe,
                        is_discard: meta.is_discard,
                    },
                    StoredPayload {
                        file: stored.file,
                        offset: stored.offset,
                        len: stored.len,
                    },
                )
                .map_err(|e| status_of(&e)),
            RingInner::Bytes(_) => Err(MediawayStatus::InvalidState),
        })
    };
    ok_or_status(result)
}

/// The longest clip [`mediaway_replay_ring_clip_last`] can return right now, in milliseconds:
/// how far back the oldest cut point is from the newest packet. Zero before the anchor's first
/// keyframe.
///
/// # Safety
///
/// `ring` must be a live pointer returned by [`mediaway_replay_ring_create`]. `out_span_ms` must
/// be a valid, writable, non-null out-parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_replay_ring_span_ms(
    ring: *mut ReplayRingHandle,
    out_span_ms: *mut u64,
) -> MediawayStatus {
    if out_span_ms.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: forwarded function contract.
    let result = unsafe {
        with_ring(ring, |inner| {
            Ok(to_millis(match inner {
                RingInner::Bytes(r) => r.span(),
                RingInner::Stored(r) => r.span(),
            }))
        })
    };
    match result {
        Ok(ms) => {
            // SAFETY: `out_span_ms` is checked non-null above (function contract).
            unsafe { out_span_ms.write(ms) };
            MediawayStatus::Ok
        }
        Err(status) => status,
    }
}

/// Cut the last `span_ms` of every stream at a keyframe.
///
/// The clip starts at the latest anchor keyframe at or before `newest - span_ms`: up to one
/// keyframe interval earlier than asked, never later; with less than `span_ms` held it starts at
/// the oldest keyframe.
///
/// `*out_has == false` (and `*out_clip == NULL`) until the anchor's first keyframe has been
/// pushed. Otherwise `*out_clip` is an **owned snapshot**: keep pushing while you read it, and
/// release it with [`mediaway_replay_clip_free`].
///
/// # Safety
///
/// `ring` must be a live pointer returned by [`mediaway_replay_ring_create`]. `out_clip` and
/// `out_has` must be valid, writable, non-null out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_replay_ring_clip_last(
    ring: *mut ReplayRingHandle,
    span_ms: u64,
    out_clip: *mut *mut ReplayClipHandle,
    out_has: *mut bool,
) -> MediawayStatus {
    if out_clip.is_null() || out_has.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: both are checked non-null above and writable per the function contract.
    unsafe {
        out_clip.write(std::ptr::null_mut());
        out_has.write(false);
    }
    let span = Duration::from_millis(span_ms);
    // SAFETY: forwarded function contract.
    let result = unsafe {
        with_ring(ring, |inner| {
            Ok(match inner {
                RingInner::Bytes(r) => r.clip_last(span).map(|clip| ReplayClipHandle {
                    duration: clip.duration(),
                    entries: clip
                        .entries()
                        // clone: the ring keeps its copy for later clips; `Bytes` is refcounted,
                        // so this is a refcount bump, not a copy of the frame.
                        .map(|(meta, p)| (meta, ClipPayload::Bytes(p.clone())))
                        .collect(),
                }),
                RingInner::Stored(r) => r.clip_last(span).map(|clip| ReplayClipHandle {
                    duration: clip.duration(),
                    entries: clip
                        .entries()
                        .map(|(meta, p)| (meta, ClipPayload::Stored(*p)))
                        .collect(),
                }),
            })
        })
    };
    match result {
        Ok(Some(clip)) => {
            // SAFETY: both are checked non-null above (function contract).
            unsafe {
                out_clip.write(Box::into_raw(Box::new(clip)));
                out_has.write(true);
            }
            MediawayStatus::Ok
        }
        Ok(None) => MediawayStatus::Ok,
        Err(status) => status,
    }
}

/// Number of packets in `clip`; `0` for a NULL clip.
///
/// # Safety
///
/// `clip` must be null or a live pointer returned by [`mediaway_replay_ring_clip_last`].
#[unsafe(no_mangle)]
pub const unsafe extern "C" fn mediaway_replay_clip_packet_count(
    clip: *const ReplayClipHandle,
) -> usize {
    if clip.is_null() {
        return 0;
    }
    // SAFETY: caller guarantees `clip` is a valid, live handle pointer (function contract).
    unsafe { &*clip }.entries.len()
}

/// The clip's length in milliseconds: from the cut keyframe to the newest packet pushed when the
/// clip was taken.
///
/// # Safety
///
/// `clip` must be a live pointer returned by [`mediaway_replay_ring_clip_last`]. `out_ms` must be
/// a valid, writable, non-null out-parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_replay_clip_duration_ms(
    clip: *const ReplayClipHandle,
    out_ms: *mut u64,
) -> MediawayStatus {
    if clip.is_null() || out_ms.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `clip` is a valid, live handle pointer (function contract);
    // `out_ms` is checked non-null above.
    unsafe { out_ms.write(to_millis((&*clip).duration)) };
    MediawayStatus::Ok
}

/// Read packet `index` of `clip`, in decode order across streams, with `pts`/`dts` rebased so the
/// cut keyframe decodes at zero. `INVALID_ARGUMENT` when `index` is out of range.
///
/// Push these through a muxer (`mediaway_muxer_add_*_track` once per stream, then
/// `mediaway_muxer_push_packet` per entry) to write the clip as a standalone file. For a `Bytes`
/// ring `entry.payload` is borrowed from the clip and valid until [`mediaway_replay_clip_free`].
///
/// # Safety
///
/// `clip` must be a live pointer returned by [`mediaway_replay_ring_clip_last`]. `out_entry` must
/// be a valid, writable, non-null out-parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_replay_clip_packet_at(
    clip: *const ReplayClipHandle,
    index: usize,
    out_entry: *mut MediawayReplayClipEntry,
) -> MediawayStatus {
    if clip.is_null() || out_entry.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `clip` is a valid, live handle pointer (function contract).
    let clip = unsafe { &*clip };
    let Some((meta, payload)) = clip.entries.get(index) else {
        return MediawayStatus::InvalidArgument;
    };
    let mut entry = MediawayReplayClipEntry {
        stream_id: meta.stream_id,
        pts: meta.pts,
        dts: meta.dts,
        duration: meta.duration,
        is_keyframe: meta.is_keyframe,
        is_discard: meta.is_discard,
        payload_kind: MediawayReplayPayloadKind::Bytes,
        payload: std::ptr::null(),
        payload_len: 0,
        stored_file: 0,
        stored_offset: 0,
        stored_len: 0,
    };
    match payload {
        ClipPayload::Bytes(bytes) => {
            entry.payload = if bytes.is_empty() {
                std::ptr::null()
            } else {
                bytes.as_ptr()
            };
            entry.payload_len = bytes.len();
        }
        ClipPayload::Stored(stored) => {
            entry.payload_kind = MediawayReplayPayloadKind::Stored;
            entry.stored_file = stored.file;
            entry.stored_offset = stored.offset;
            entry.stored_len = stored.len;
        }
    }
    // SAFETY: `out_entry` is checked non-null above (function contract).
    unsafe { out_entry.write(entry) };
    MediawayStatus::Ok
}

/// Free a clip. Always safe to call, including with `NULL`. Invalidates every `payload` pointer
/// read from it.
///
/// # Safety
///
/// `clip` must be null or a pointer returned by [`mediaway_replay_ring_clip_last`], not already
/// freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_replay_clip_free(clip: *mut ReplayClipHandle) {
    if clip.is_null() {
        return;
    }
    // A panic during drop is swallowed and the allocation leaked, as for every other handle.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `clip` is a valid, not-yet-freed handle pointer.
        drop(unsafe { Box::from_raw(clip) });
    }));
}

/// Close a ring. Always safe to call, including on a poisoned handle or with `NULL`. Clips taken
/// from it stay valid: they own their data.
///
/// # Safety
///
/// `ring` must be null or a pointer returned by [`mediaway_replay_ring_create`], not already
/// closed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_replay_ring_close(ring: *mut ReplayRingHandle) {
    if ring.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `ring` is a valid, not-yet-closed handle pointer.
        drop(unsafe { Box::from_raw(ring) });
    }));
}

#[cfg(test)]
#[path = "replay_tests.rs"]
mod tests;
