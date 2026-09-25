# ADR-0004: Replay ring — encoded packets, cut in decode order at anchor keyframes

- **Status**: Accepted
- **Date**: 2026-09-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-container`

## Context

A capture application wants "save the last five minutes" without having recorded to disk.
That needs the recent past held in memory, and it can only be the *encoded* stream: five
minutes of 1080p60 BGRA is roughly 700 GB, while HEVC at 20 Mbps is roughly 750 MB. Nothing
in mediaway kept it. The first caller is qarec (a game-QA capture tool built on mediaway),
whose replay hotkey needs it.

Two properties make this more than a `VecDeque`:

- **A clip can only start where a decoder can start**, at a keyframe. The ring has to know
  where those are and must never evict its way past the last one inside the window.
- **Reordering codecs.** H.264 and HEVC with B-frames emit packets in decode order, which is
  not presentation order. A cut chosen by presentation time can strand a frame whose
  reference is on the other side of the cut: a clip that muxes cleanly and then falls apart
  on playback. Open-GOP streams (HEVC CRA with RASL pictures) add *leading pictures*: decoded
  after a keyframe, presented before it, and allowed to reference the previous GOP.

## Decision

> We add `mediaway_container::replay::ReplayRing`, a sans-io ring of `mediaway_common::Packet`s
> for one or more streams, cut at keyframes of one **anchor** stream, in **decode order**.

- **Placement.** A module in `mediaway-container`, not a new unprefixed core. It is written
  against `Packet` and `Rational`, which unprefixed cores may not depend on (ADR-0012), and a
  generic core would be an abstraction with one caller. Its output feeds a muxer, so it sits
  with the muxers. No I/O and no clock: the caller pushes packets and asks for clips.
- **Anchor.** One stream (normally video) supplies cut points: its keyframes. Other streams
  (audio) are cut by time to match. Every stream's packets must arrive in non-decreasing
  `dts`; a regression is an error, not a silent reorder.
- **Cut.** `clip_last(span)` picks the latest anchor keyframe *K* whose decode time is at or
  before `newest - span`, or the oldest one held when the ring is shorter than `span`. A clip
  therefore starts up to one keyframe interval *earlier* than asked, never later. The clip
  holds:
  - anchor packets with `dts >= K.dts`, **except** those with `pts < K.pts` (leading
    pictures, whose references may be gone);
  - other streams' packets from the first one decoded at or after *K*'s presentation time.
    Since `pts >= dts`, none of those presents before *K*.
- **Rebasing.** Timestamps are shifted so *K* decodes at zero: the anchor by `K.dts` exactly,
  other streams by the same instant converted to their timebase (rounded to nearest). Packets
  come out interleaved across streams by decode time, anchor first on ties.
- **Eviction.** Whole GOPs, from the front, and only once the *second* oldest keyframe is
  itself at least `window` behind the newest packet. A cut point at or before the window start
  therefore always survives, and memory holds between `window` and `window` plus one GOP.
  Other streams are trimmed to the new first keyframe's presentation time.
- **Byte ceiling.** `with_max_bytes` also evicts GOPs while payload bytes exceed a ceiling,
  inside the window if need be. The newest GOP always stays, so one GOP larger than the
  ceiling exceeds it.
- **Anchor packets before the first keyframe are dropped**: nothing can decode from them.
- **Cost.** A clip borrows the ring. Each output packet is a `Packet` clone whose payload is a
  refcounted `Bytes`: a refcount bump, not a payload copy. Per stream, a `VecDeque` grows to
  the window and is then reused. Cut points live in their own `VecDeque`, so eviction never
  scans the packet queues for keyframes.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Cut and evict by presentation time | Strands B-frames and leading pictures across the cut; the clip decodes wrong. The reason this ADR exists. |
| Keep leading pictures in the clip | Correct for closed GOPs, broken for open GOPs (RASL), and the ring cannot tell which it has. Dropping them costs at most a few frames presented before *K*. |
| New unprefixed core, generic over a packet trait | Cores may not use `Packet`/`Rational`, and the genericity would serve no second caller. |
| In the `mediaway` umbrella next to `EncodeSession` | It is pure packet bookkeeping with no encoder or device, and a caller that only muxes should not need the umbrella. |
| One interleaved queue for all streams | Evicting by push order loses audio pushed slightly before a keyframe it belongs after; per-stream queues evict each by its own rule. |
| `SmallVec` for the track list | Allocated once at construction, never on the packet path; `Vec` avoids a new dependency for no measurable gain. |
| Store decoded frames | Three orders of magnitude more memory. |

## Consequences

### Positive

- "Save the last N seconds" is a `clip_last` plus a fresh muxer, and the file starts at a
  decodable keyframe at time zero.
- Correct for reordering and open-GOP encoders by construction, and tested with an open-GOP
  decode-order sequence.

### Negative / Trade-offs

- **Clip precision is the keyframe interval.** The caller must drive the encoder with a
  bounded keyframe interval, or a clip can start arbitrarily early (and the ring holds up to
  one GOP beyond the window). Shorter intervals cost bitrate.
- **Leading pictures at the cut are lost**, even where the stream is closed-GOP and they would
  have decoded. A few frames at most.
- **Codec configuration is the caller's job.** The ring holds packets only; the new muxer's
  tracks still need their `extra_data` (e.g. `hvcC`), which the caller has from the encoder.
- **Memory is `window` plus one GOP**, or the byte ceiling. At 20 Mbps over five minutes,
  about 750 MB.
- A clip borrows the ring, so the caller writes it out before pushing more. If holding pushes
  that long would back up the encoder, collect `packets()` into a `Vec` first (refcount bumps
  only) and write from that.

## Updates

### 2026-09-25 — Payloads may be stored elsewhere (`ReplayRing<P = Bytes>`)

The first caller writes the same packets to a fragmented MP4 while the ring holds them, so the
ring's ~450 MB (12 Mbps, five minutes; ceiling 2x) duplicated bytes already on disk. The ring is
now generic over what it keeps of a payload:

- `ReplayRing<P = Bytes>`, `P: ReplayPayload` (`fn byte_len(&self) -> usize`), implemented for
  `Bytes` and for `StoredPayload { file: u32, offset: u64, len: u32 }`, a `Copy` location whose
  `file` id is the caller's. Entries are `PacketMeta` (every `Packet` field but the payload,
  `Copy`) plus `P`. Static dispatch only; no new dependency.
- `Bytes` API unchanged: `new`, `push(Packet)`, `Clip::packets()`. `new` stays `Bytes`-only
  (as `HashMap::new` is `RandomState`-only) so existing calls need no annotation; any `P` is
  built with `for_payload`, fed with `push_entry(PacketMeta, P)`, and read with
  `Clip::entries()`, which yields `(PacketMeta, &P)` rebased, in the same order `packets()`
  uses (`packets()` is now `entries()` plus a `Bytes` refcount bump).
- `payloads()` iterates every payload still held, so a caller can free storage nothing refers
  to any more (e.g. delete spill files).
- `with_max_bytes` counts `byte_len()`: for `StoredPayload` it bounds referenced *disk* bytes;
  the ring's own memory is then a few dozen bytes per packet.
- Byte positions come from `iso-bmff`'s opt-in mux placements
  (`iso-bmff/adr/0007-mux-payload-placements.md`). Reading bytes back is the caller's I/O; the
  ring stays sans-io. Eviction and cutting are identical for every `P`, tested by running the
  same reordering A/V sequence through a `Bytes` ring and a `StoredPayload` ring.

Trade-off: a stored clip is only as durable as the caller's files. Deleting storage that
`payloads()` still reports, or that a collected clip still refers to, breaks the clip.

## References

- `src/replay.rs`, `src/replay_tests.rs`
- `docs/ai/wiki/container/replay-ring.md`
- Workspace ADR-0012 (unprefixed cores); `iso-bmff/adr/0004` (sample durations from dts deltas)
