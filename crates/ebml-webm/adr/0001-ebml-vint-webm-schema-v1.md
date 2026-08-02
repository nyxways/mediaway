# ADR-0001: EBML VINT + WebM demux schema subset (v1)

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `ebml-webm`

## Context

Mediaway needs a second container format behind `mediaway-container`'s
`Mux`/`Demux` traits (roadmap Stage 2). Full EBML + Matroska/WebM is a large
spec (VINT encoding, an open-ended element tree, lacing, seek/cue tables,
mux). Doing all of it in one pass is not finishable or reviewable. This ADR
scopes a v1 slice that is real (parses actual WebM structure, extracts
packets) and honest about what is deferred.

Standards referenced (URL + registry, no pasted text):
[RFC 8794](https://www.rfc-editor.org/rfc/rfc8794) (EBML) and the
[WebM container guidelines](https://www.webmproject.org/docs/container/) —
see `docs/standards/registry.toml` (`rfc-8794-ebml`, `webm-container-guidelines`).

## Decision

> One unprefixed crate `ebml-webm` (no `mediaway-common` dependency) owns EBML
> VINT parsing and a WebM-schema demuxer. **Demux only** for v1; mux is a
> follow-up stage. Product wiring lives in `mediaway-container::webm`
> (facade ADR: `mediaway-container/adr/0001`).

1. **Low-level VINT API is public and first-class** (ADR-0003 rule: low-level
   APIs stay usable, not buried): `vint::decode_size` / `vint::decode_id`
   are plain functions over `&[u8]`, returning `Result<_, Error>` —
   `Error::Incomplete` (need more bytes) is distinct from
   `Error::ReservedVint` (malformed marker, unrecoverable at this position).
2. **Element subset walked**: `EBML` header (id `0x1A45DFA3`, skipped whole —
   no `DocType` validation in v1), `Segment` (`0x18538067`),
   `Segment\Info` (`0x1549A966`) for `TimecodeScale` (`0x2AD7B1`),
   `Tracks` (`0x1654AE6B`) / `TrackEntry` (`0xAE`) for `TrackNumber` (`0xD7`),
   `TrackType` (`0x83`), `CodecID` (`0x86`, ASCII), `Video` (`0xE0`) for
   `PixelWidth`/`PixelHeight` (`0xB0`/`0xBA`), `Cluster` (`0x1F43B675`) for
   `Timecode` (`0xE7`) and `SimpleBlock` (`0xA3`).
3. **Unknown-size (indefinite) elements**: allowed only for the master
   elements the walker descends into (`Segment`, `Cluster`, …). Such a
   context stays open until its parent closes or EOF — no sibling-ID
   lookahead to end it early. Correct for typical file/VOD WebM (definite-size
   `Cluster`); a long-running indefinite-size live stream can grow the
   open-element stack unboundedly (documented gap, roadmap Stage 1 deferred).
   An indefinite size on an element **not** in the descend list is treated as
   unrecoverable (`halted`, see below) — the walker has no way to know where
   it ends.
4. **`SimpleBlock` only** — track number (size-style VINT, marker stripped,
   per the WebM block-header convention), 2-byte signed relative timecode,
   1 flag byte (keyframe bit `0x80`). Non-zero lacing bits → frame dropped,
   not decoded (documented gap). `BlockGroup`/`Block`/`BlockDuration` are not
   read; per-frame `duration` is always `0` until that lands.
5. **No panics on untrusted bytes** — `push_bytes`/`poll_frame` never panic.
   A structurally unrecoverable position (reserved VINT marker, indefinite
   size on a non-descend element) sets an internal `halted` flag and the
   demuxer stops advancing; already-extracted tracks/frames are kept. There is
   **no `Result` on `push_bytes`/`poll_frame`** (mirrors `iso_bmff::Demuxer`) —
   the `Error` type is for the public low-level parse functions, not the
   stateful demuxer loop.
6. **Codec identity stays a string** (`TrackInfo::codec_id`, e.g. `"V_VP9"`).
   Mapping to Mediaway `CodecKind` is the facade's job — `CodecKind` has no
   `Vp8`/`Vorbis` variants today, so those "real WebM" codecs cannot round-trip
   through the facade yet (facade ADR documents the drop).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Full Matroska (not just WebM profile) in the same pass | Much larger ID/semantic surface; WebM is the actual product need |
| Implement mux alongside demux | Doubles scope; demux is independently useful and verifiable now |
| Sibling-ID lookahead to close indefinite `Cluster` precisely | Real but non-trivial (needs a small pushdown/backtrack); deferred rather than half-done |
| `mediaway-container-webm` as its own crate | Violates ADR-0012 / crate-packaging: thin adapters live in the facade |
| Push codec→`CodecKind` mapping into `ebml-webm` | Would require `mediaway-common` dependency, breaking the unprefixed-core rule |

## Consequences

### Positive

- Real, testable WebM demux (EBML VINT + Segment/Tracks/Cluster/SimpleBlock)
  with no `mediaway-common` coupling, reusable outside Mediaway.
- Low-level VINT functions are independently useful (probe/debug tooling).

### Negative / Trade-offs

- Lacing, `BlockGroup`/`BlockDuration`, and indefinite-`Cluster` lookahead are
  real gaps, not just naming — some legitimate WebM files (laced audio,
  live-streamed indefinite clusters) will lose or drop frames in v1.
- VP8/Vorbis tracks are demuxed structurally but invisible through the
  Mediaway-typed facade until `CodecKind` grows those variants.

## References

- `docs/adr/0003-crate-packaging.md`, `docs/adr/0012-unprefixed-reusable-cores.md`
- `mediaway-container/adr/0001-webm-ebml-demux.md`
- `docs/standards/registry.toml` (`rfc-8794-ebml`, `webm-container-guidelines`)
