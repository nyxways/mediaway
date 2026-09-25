# Replay ring

`mediaway_container::replay::ReplayRing`. ADR:
[`mediaway-container/adr/0004`](../../../../crates/mediaway-container/adr/0004-replay-ring.md).

Holds the last `window` of *encoded* packets for one or more streams, and answers
`clip_last(span)` with a packet sequence a fresh muxer can write as a standalone file.
Sans-io: no I/O, no clock; the caller pushes and asks.

## Rules worth knowing before touching it

- **Anchor stream** (the one passed to `new`) supplies cut points: its keyframes only.
  Other streams (`add_stream`) are cut by time.
- **Decode order everywhere.** Per-stream `dts` must not decrease (`OutOfOrder` otherwise).
  Cut selection and eviction use `dts`.
- **Cut at K** = the latest anchor keyframe with decode time `<= newest - span` (or the
  oldest held). Clips start up to one GOP early, never late.
- **Leading pictures are dropped**: anchor packets after K with `pts < K.pts` (HEVC RASL,
  open GOP). Other streams start at the first packet decoded at or after K's presentation
  time, and since `pts >= dts` need no further filter.
- **Rebase**: K decodes at 0. The anchor shifts by `K.dts`; others by the same instant in
  their own timebase, rounded to nearest.
- **Eviction** drops whole GOPs once the *second* keyframe is `window` old, so a cut point
  always exists at the window start. Memory is the window plus up to one GOP, or
  `with_max_bytes` (the newest GOP is always kept).
- Anchor packets before the first keyframe are discarded on push.
- A `Clip` borrows the ring. Its packets are `Packet` clones: a `Bytes` refcount, no copy.

## Payload type: `ReplayRing<P = Bytes>`

- `P: ReplayPayload` (`byte_len`). `Bytes` (default) or `StoredPayload { file, offset, len }`:
  where the bytes sit in one of the caller's files. The ring never reads it.
- `Bytes`: `new` / `push(Packet)` / `Clip::packets()` (unchanged). Any `P`:
  `ReplayRing::<P>::for_payload` / `push_entry(PacketMeta, P)` / `Clip::entries()` →
  `(PacketMeta rebased, &P)`, same order as `packets()`. `new` is `Bytes`-only on purpose
  (`HashMap::new` pattern) so old call sites infer without annotations.
- `with_max_bytes` counts `byte_len()`: disk bytes referenced, for `StoredPayload`.
- `payloads()` lists every payload still held; storage absent from it is free to delete.
- Offsets come from `mp4::Muxer::with_placements` → `poll_placements` (`iso-bmff/adr/0007`):
  absolute offset in the polled byte stream, `len` of the payload *as written* (H.264/HEVC
  length-prefixed). Bytes read back are that converted form.

```mermaid
flowchart LR
  subgraph capture["encoder thread"]
    E[encoder poll] -->|push, dts order| R[(ReplayRing)]
  end
  subgraph save["on request"]
    R -->|clip_last span| C[Clip cut at K]
    C -->|rebased, interleaved by dts| M[new muxer]
    X[caller: track extra_data] --> M
  end
```

## Caller obligations

- Force a bounded keyframe interval on the encoder: it is the clip's precision.
- Register the new muxer's tracks with codec config (`extra_data`) from the encoder. The
  ring holds packets only.

Tests: `crates/mediaway-container/src/replay_tests.rs` covers an open-GOP decode-order
sequence, two timebases interleaved, the byte ceiling, the eviction boundary, and a `Bytes`
ring vs a `StoredPayload` ring fed the same A/V sequence (identical clips, bytes, payloads).
