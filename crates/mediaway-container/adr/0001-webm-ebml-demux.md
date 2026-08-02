# ADR-0001: WebM demux via a new unprefixed `ebml-webm` core

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-container`

## Context

Roadmap Stage 2 (`docs/roadmap.md` / crate `docs/roadmap.md`) asks for a
second container format behind the existing `ContainerFormat`/`Mux`/`Demux`/
`DemuxDecrypt` traits, currently only wired to MP4 (`mp4` module over
`iso-bmff`). Per ADR-0012 and `docs/spec/crate-packaging.md`, a new container
format with a freestanding core gets an **unprefixed crate** + a **thin
facade adapter module** — not a giant module inside this facade, and not a
separate `mediaway-container-webm` crate.

Full WebM mux+demux is out of scope for one session; see the new crate's own
`ebml-webm/adr/0001` for the exact element subset and deferred items.

## Decision

> New unprefixed crate **`ebml-webm`** (EBML tokenizer + WebM schema subset,
> demux only) plus a thin `mediaway-container::webm` adapter module — same
> shape as `iso-bmff` + `mediaway-container::mp4`.

1. **Naming**: `ebml-webm`, not `webm` or `matroska`. WebM is EBML-based; the
   name signals both the low-level tokenizer (VINT/element walk, reusable for
   any EBML doc type) and the schema layer actually implemented (WebM's
   Segment/Tracks/Cluster/SimpleBlock subset, not general Matroska). If a real
   non-WebM Matroska need appears later, split with a new ADR — do not grow
   this crate's schema surface silently.
2. **`Demux` only** — `mediaway_container::webm::Demuxer` implements this
   crate's `Demux` trait. `Mux` and `ContainerFormat` are **not** implemented
   yet (mirrors how `mp4`'s `ContainerFormat` impl is cfg-gated on both `mux`
   and `demux` features being enabled); adding a `webm` `Mux` impl is a
   follow-up once `ebml-webm` grows a muxer.
3. **Feature gating** — the `webm` module is behind this crate's existing
   `demux` feature (same feature MP4 demux already uses), not a new feature
   flag. `ebml-webm` itself has no cargo features yet (demux-only crate).
4. **Codec mapping gap is explicit, not silent** — `ebml-webm::TrackInfo`
   carries the raw WebM `CodecID` string (e.g. `"V_VP9"`, `"V_VP8"`).
   `webm::Demuxer::streams()` maps to Mediaway `StreamInfo`/`CodecKind` only
   for codecs `CodecKind` already has (`Vp9`, `Av1`, `Opus`, `Aac`). Tracks
   using `CodecKind`-less codecs (VP8, Vorbis — both common in real WebM) are
   **omitted** from `streams()` and their frames are dropped in
   `poll_packet()`. This is a real product gap (not every legal WebM file
   round-trips), tracked in both crates' roadmaps, not hidden behind a
   generic `Other` codec variant that would silently mismux downstream.
5. **No `Result` on the `Demux` trait** — `push_bytes`/`streams`/`poll_packet`
   have no error channel (same shape as `mp4`'s `Demuxer`). Malformed/
   truncated bytes and the codec-mapping gap above are handled by dropping
   data cleanly (never panicking), documented at the item, not by inventing a
   `Result` return the trait doesn't have.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| WebM logic as a module directly inside `mediaway-container` | Couples EBML parsing to Mediaway types; violates ADR-0012 |
| New `mediaway-container-webm` crate | Adapter is thin (one module); no ADR-justified reason to split from the facade yet |
| Name the core crate `webm` | Undersells that the tokenizer half (VINT/element walk) is generic EBML, not WebM-specific |
| Add a `CodecKind::Vp8`/`Vorbis` variant now to "complete" the mapping | Out of scope for this session; `CodecKind` is shared by every facade — changing it needs its own review, not a drive-by from this ADR |

## Consequences

### Positive

- Same proven pattern as `iso-bmff`/`iso-cenc`: reusable core, thin facade.
- `Demux` trait gets a second real implementation, exercising it beyond MP4.

### Negative / Trade-offs

- VP8/Vorbis WebM files demux structurally in `ebml-webm` but produce no
  packets through the Mediaway-typed facade until `CodecKind` is extended.
- No mux yet — `mediaway-container::webm` is demux-only.

## References

- `ebml-webm/adr/0001-ebml-vint-webm-schema-v1.md`
- `docs/adr/0003-crate-packaging.md`, `docs/adr/0012-unprefixed-reusable-cores.md`
- crate `docs/roadmap.md` (Stage 2)
