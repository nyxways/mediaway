# ADR-0002: Lacing, `BlockGroup`, `Audio` fields, `Cues`/`SeekHead` (full Matroska profile elements)

- **Status**: Accepted
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `ebml-webm`

## Context

ADR-0001's v1 demux deliberately dropped laced frames, never read
`BlockGroup`/`BlockDuration`/`ReferenceBlock`, ignored `Audio\SamplingFrequency`/
`Channels`, and didn't parse `Cues`/`SeekHead`. These are real Matroska/WebM
features (Matroska is a superset container profile; WebM restricts codecs and a
few structural choices but shares the same EBML element family) used by
real-world files, not WebM-specific extensions.

## Decision

> Extend `ebml-webm`'s existing demux in place — no new crate. Matroska and
> WebM share the same EBML container; a second `ebml-matroska` crate would
> duplicate the VINT walker and element-tree machinery for no real separation
> (the "container layout" principle avoids format-shim proliferation).

1. **Lacing** (`lacing.rs`, new module): Xiph, fixed-size, and EBML lacing are
   all implemented (not just Xiph) — a block's flags byte fully determines the
   lacing mode, and leaving any of the three unimplemented would silently drop
   real-world files using that mode. Malformed lace data returns `None` from
   `lacing::split`, which the caller treats as "drop this block", matching the
   crate's existing "never panic, drop cleanly" posture (ADR-0001 §5). Laced
   sub-frames share one timecode (Matroska doesn't encode a distinct timecode
   per sub-frame) — a real spec property, not an approximation invented here.
2. **`BlockGroup`/`Block`/`BlockDuration`/`ReferenceBlock`**: `Block` reuses the
   same wire-format parser as `SimpleBlock` (`parse_block_common`), but
   keyframe-ness comes from the *absence* of a `ReferenceBlock` sibling rather
   than a flag bit (`Block`'s top bit is reserved, unlike `SimpleBlock`).
   `Frame` gained `duration_ticks: Option<u64>` (from `BlockDuration`,
   `None` for a bare `SimpleBlock`, which carries no duration).
3. **`Audio\SamplingFrequency`/`Channels`**: `TrackInfo` gained `sample_rate: f64`
   and `channels: u32`, defaulting to the spec values (`8000.0`, `1`) when
   absent — not `0.0`/`0`, which would misrepresent an unset field as silence.
   `SamplingFrequency` is an EBML "Float" (4- or 8-byte IEEE-754 BE); a new
   `read_float` helper mirrors the existing `read_uint`.
4. **`Cues`/`SeekHead`**: parsed and exposed as informational
   (`Demuxer::cues()`/`seek_head()`) — this crate does **no seeking itself**
   (sans-io: I/O and seek-driven re-reads belong to the adapter layer, per
   `docs/spec/sans-io.md`). A caller with real file I/O can use these to jump
   directly to a `Cluster` byte offset; `ebml-webm` only hands over the parsed
   index.
5. Still deferred (unchanged from ADR-0001): sibling-ID lookahead to close an
   indefinite-size `Cluster` early, and mux (writer). Multi-packet PSI-style
   concerns don't apply here (that's an MPEG-TS thing), but the open-element
   stack's indefinite-size handling is the same known gap.

## Consequences

- `TrackInfo` and `Frame` are additive-only breaking changes (new fields) —
  `mediaway-container::webm`'s existing exhaustive struct literals (test
  fixtures) needed updating; production code there only reads fields, unaffected.
- `TrackInfo` dropped `Eq` (kept `PartialEq`) since `f64` has no total order —
  a real, unavoidable consequence of adding `sample_rate`.
- No behavior change for existing non-laced, non-`BlockGroup`, non-`Audio`,
  non-`Cues`/`SeekHead` WebM files — all pre-existing tests pass unchanged.

## References

- `crates/ebml-webm/adr/0001-ebml-vint-webm-schema-v1.md` — v1 scope this extends
- [`docs/spec/sans-io.md`](../../../docs/spec/sans-io.md) — why seeking stays out of this crate
- `docs/standards/registry.toml` (`rfc-8794-ebml`, `webm-container-guidelines`)
