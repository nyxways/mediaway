# ADR-0003: WebM mux (`Segment`/`Tracks`/`Cluster`/`SimpleBlock` writer)

- **Status**: Accepted
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `ebml-webm`

## Context

`adr/0001` scoped v1 to demux-only, deferring mux "once a concrete product
need appears" (roadmap Stage 3). README.md's Container support table still
marked WebM mux 🛠️ (planned, never started). This ADR adds a real muxer
mirroring `iso_bmff::mux`'s typestate shape (`Open` → `Live`,
`output`/`poll_bytes` drain pattern) so `mediaway-container::webm` can grow a
`Mux` impl alongside its existing `Demux` one.

## Decision

> Add `ebml_webm::mux::Muxer<Open | Live>` — register tracks, `begin()`,
> `push_frame`, `poll_bytes`. Scope matches what the demuxer already reads
> back: `EBML` header, `Segment` (always unknown-size — streaming-first, no
> upfront total-length requirement), `Segment\Info\TimecodeScale`,
> `Tracks\TrackEntry` (`TrackNumber`/`TrackType`/`CodecID`/`Video`/`Audio`
> sub-fields), `Cluster\Timecode` + `SimpleBlock` (no lacing, no
> `BlockGroup` — a muxer never *needs* to lace or defer a keyframe decision
> the way some encoders do; those stay demux-only features per `adr/0002`).

1. **`Cluster` batching, not unknown-size clusters.** Each `Cluster` is
   written with a *known* size: frames buffer into a scratch `Vec<u8>` and
   the `Cluster` header (with `vint::encode_size`) is only emitted once the
   batch closes (`DEFAULT_CLUSTER_BATCH` frames, or a relative timecode that
   would overflow `SimpleBlock`'s signed 16-bit offset field, or an explicit
   `flush()`). Simpler than indefinite-size `Cluster` + a `Void` filler or
   sibling-ID lookahead, and every `Cluster` this muxer emits is trivially
   re-parseable without the demux side's "stays open until EOF" fallback
   (`adr/0001`'s documented gap doesn't apply to files this crate itself
   writes).
2. **Track registration happens before `begin()`** (`Open` state) — once
   live, `Tracks` has already been written and cannot gain a new entry
   mid-stream (matches real Matroska: `Tracks` isn't supposed to change after
   the first occurrence in a `Segment`).
3. **`vint::encode_id`/`encode_size`/`encode_unknown_size` are new public,
   low-level functions** alongside the existing `decode_id`/`decode_size`
   (same "low-level APIs stay first-class" rule `adr/0001` already follows).
   They are total functions (never panic on out-of-range input — saturate or
   clamp instead) even though every call site inside this crate only ever
   passes values that are already in range, because they're public API a
   caller could misuse.
4. **No re-lacing, no `BlockGroup` output.** The muxer always writes one
   `SimpleBlock` per pushed frame (no lacing) — real encoders don't typically
   need Matroska's lacing (it exists to save a few bytes of per-block
   overhead for tiny audio frames); adding a lacing *writer* is future work
   if a real product need appears, not implemented speculatively here.
5. **Verification**: no external WebM mux oracle is practical to add this
   session (unlike demux, which has real FATE samples to compare against).
   The mux path is verified by round-tripping through this crate's own
   `Demuxer` (`mux_tests.rs`) — the strongest check available without a
   second independent WebM implementation in the loop; `ffprobe` oracle
   comparison stays a demux-only technique (`docs/conventions/testing.md`
   § FATE corpus).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Unknown-size `Cluster` (matching `Segment`) | Needs either sibling-ID lookahead or a `Void` filler to close cleanly; known-size clusters are simpler and this crate controls exactly what it writes |
| Known-size `Segment` (buffer everything, write size upfront) | Breaks streaming-first (`docs/spec/async-and-streaming.md`); unknown-size `Segment` is legal EBML and every real WebM player already handles it (live-streamed WebM always does this) |
| Support `BlockGroup`/lacing on the mux side | No real need yet — the demuxer already reads both for *other* encoders' output; this muxer's own output doesn't require them |
| External oracle (`ffprobe`-write comparison, or install a WebM validator) | No local dev/test-oracle WebM validator tool is part of this workspace's toolchain; round-trip-through-own-demuxer is the practical near-term check |

## Consequences

### Positive

- `mediaway-container::webm` can gain a real `Mux` impl without inventing new
  EBML-writing logic there — the low-level writer lives in the freestanding
  core, matching every other container crate's mux/demux split.
- `vint::encode_id`/`encode_size` are independently useful (any future EBML
  writer, not just `WebM`, could reuse them — they know nothing about the
  `WebM` schema).

### Negative / Trade-offs

- No lacing writer — a caller pushing many tiny frames (e.g. very short audio
  packets) pays one full `SimpleBlock` header per frame instead of amortizing
  over a laced group. Not a correctness gap, a size-efficiency one.
- Mux output has not been validated against a real external player/muxer
  (only round-tripped through this crate's own demuxer) — a latent risk that
  some subtle wire-format assumption this crate's demux and mux sides happen
  to agree on diverges from the actual Matroska spec in a way neither side's
  tests would catch. Mitigated by both sides being written directly against
  RFC 8794 / the WebM container guidelines (same standards registry entries
  as `adr/0001`), not against each other.

## References

- `adr/0001-ebml-vint-webm-schema-v1.md`, `adr/0002-full-matroska-profile.md`
- `crates/iso-bmff/src/mux/mod.rs` (typestate + `output`/`poll_bytes` pattern reused here)
- `docs/standards/registry.toml` (`rfc-8794-ebml`, `webm-container-guidelines`)
