# ADR-0009: Replay ring and MP4 payload placements over the container C ABI

- **Status**: Accepted — hardware-verified through the C ABI on real WMF H.264 and through the C++, C#, Python and Node.js bindings
- **Date**: 2026-09-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-ffi` (container domain)

## Context

`mediaway_container::replay::ReplayRing` (`mediaway-container` ADR-0004) keeps the last *N* seconds
of encoded packets and cuts "the last *M* seconds" at a keyframe, in decode order, as a packet
sequence rebased to zero that a fresh muxer writes as a standalone file. It is generic over its
payload: `Bytes` (refcounted, in memory) or `StoredPayload { file, offset, len }` (where the caller
already wrote the bytes). `iso_bmff::Muxer::with_placements` supplies those locations
(`iso-bmff` ADR-0007). None of it is reachable from C, C++, C#, Python or Node.

**A limit to state first:** the C ABI has no video-packet source. `mediaway_encode_session` muxes
its encoder's packets internally, and packet-level output exists only for the audio encoder. A C
ring is therefore fed by demuxer packets, audio-encoder packets, or packets from an encoder the
caller drives itself. A C video-packet encoder API is a separate change and is not designed here.

## Decision

> Add an opaque replay ring, an owned clip snapshot, and MP4 muxer placements.
> `MEDIAWAY_CONTAINER_FFI_ABI_VERSION` goes 7 → 8. Additions only; no existing struct changes.

### 1. Ring

`mediaway_replay_ring_t` is created from a plain value config (`anchor_stream_id`,
`anchor_time_base`, `window_ms`, `max_bytes` with `0` = no ceiling, `payload_kind`). Inside is
`enum { Bytes(ReplayRing<Bytes>), Stored(ReplayRing<StoredPayload>) }`: a closed set, so enum
dispatch and no `Box<dyn>`. The handle has the usual `poisoned` guard.

- `push(ring, packet_view)` is for a `BYTES` ring. The view is borrowed for the call, so the payload
  is copied once into a `Bytes`; every clip then shares that copy by reference count.
- `push_stored(ring, meta, stored)` is for a `STORED` ring: metadata plus `{file, offset, len}`. The
  ring never reads the file and holds a few dozen bytes per packet.
- The wrong kind for the ring is `INVALID_STATE`.
- `add_stream`, `span_ms`, `close`. Times are **milliseconds**: a replay window is seconds, and a
  `u64` of milliseconds is exact enough that a `Duration` pair would only add a field.

### 2. Clip is an owned snapshot

The Rust `Clip` borrows the ring and must be written out before the next `push`. In C that rule
cannot be enforced and would be a use-after-free waiting to happen. `mediaway_replay_ring_clip_last`
returns a `mediaway_replay_clip_t` that owns a `Vec` of `(rebased metadata, payload)`. `Bytes` are
cloned by reference count and `StoredPayload` is `Copy`, so a clip costs one allocation and no
payload copy, and the ring may keep taking packets while a clip is read or after it is closed.

`packet_count`, `packet_at(i, &entry)` and `duration_ms` read it. A `BYTES` entry's `payload` is
borrowed from the clip and valid until `mediaway_replay_clip_free`. A `STORED` entry carries
`stored_file`, `stored_offset` and `stored_len` instead.

### 3. No "mux this clip" function

The clip's packets go through the existing muxer: `add_*_track` once per stream, then
`push_packet` per entry. That is what the Rust API does, it keeps the low-level path first-class
(`docs/spec/api-layers.md`), and a one-shot writer would need every track's codec configuration,
which the ring deliberately does not store. The C example shows the composition.

### 4. MP4 placements

`mediaway_muxer_create_with_placements` (MP4 only) returns a muxer that also records where each
sample's payload landed. `mediaway_muxer_poll_placements` returns an owned array of
`{track_id, dts, offset, len}`, freed by `mediaway_placements_free`. The output bytes are unchanged.
`len` is the payload **as written** (Annex-B becomes length-prefixed; ADTS is stripped), so it can
differ from the pushed length. `INVALID_STATE` on an `Open` muxer and on WebM, which never records; a
plain MP4 muxer records nothing and returns an empty array, which is what the Rust API does too.
Placements come back in **write** order, and a fragment writes its samples grouped by track, so with two
tracks `placements[i]` is not the i-th packet pushed: a caller matches by `(track_id, dts)`. Found when the
C++ binding's first Stored-ring example indexed by position and read the wrong bytes.

### 5. Status codes

No new values. `UnknownStream` maps to the existing `UNKNOWN_STREAM`, a duplicate stream to
`INVALID_TRACK`, a bad timebase to `INVALID_ARGUMENT`, and a `dts` that went backwards to
`INVALID_PACKET`: the caller can drop that packet and carry on, which is why it is not folded into a
generic error.

Zero-cost shape: one allocation when the ring is built, one payload copy per `BYTES` push, and one
`Vec` per clip. No `Box<dyn`, no per-packet clone beyond the `Bytes` refcount.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| A clip that borrows the ring | Unenforceable in C; a push during a read is a use-after-free |
| One `push` taking a tagged union of payload or location | Every caller pays for the case it does not use; two functions with distinct types read better |
| `clip_write_mp4(clip, muxer)` | Needs each track's codec config, which the ring does not hold; hides the composition |
| `Duration` as seconds plus nanos | A second field for precision a replay window does not need |
| A new status for out-of-order | The shared enum already has `INVALID_PACKET`, which is what the caller acts on |

## Consequences

### Positive

- A C caller can keep a rolling recording and save "the last 30 seconds" at a keyframe, and can
  keep the bytes on disk instead of in RAM.
- Nothing in the existing container surface changes.

### Negative / Trade-offs

- **No C video-packet source yet** (see Context): the ring is useful from C today with demuxed,
  audio-encoder or caller-encoded packets.
- A `BYTES` push copies the payload once.
- A clip's payload pointers die with the clip.

## References

- `mediaway-container` ADR-0004 (replay ring), `iso-bmff` ADR-0007 (placements)
- `crates/mediaway-container/src/replay.rs`, `docs/ai/wiki/container/replay-ring.md`
- ADR-0001 (handle and panic-safety pattern), pipeline ADR-0007 (probe array ownership)
