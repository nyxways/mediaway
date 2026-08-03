# ADR-0001: `riff-wave` — freestanding RIFF/WAVE PCM mux + demux

- **Status**: Accepted
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `riff-wave`

## Context

RIFF/WAVE (PCM) has no Mediaway-specific typing needs (no boxes/tracks/timescales in
the ISOBMFF sense) — it is a minimal, universally-supported audio container. Adding
it rounds out "commonly used audio containers" alongside the existing ISOBMFF/WebM
video-first containers.

## Decision

> New unprefixed freestanding crate `riff-wave` (naming: ADR-0012), sans-io, no
> Mediaway dependency — mirrors `iso-bmff`/`ebml-webm`'s freestanding-core pattern.

- Scope: PCM integer + IEEE float `fmt ` chunks only (`wFormatTag` 1 / 3). No
  `WAVE_FORMAT_EXTENSIBLE`, no compressed formats (ADPCM, µ-law/A-law, MP3-in-WAV) —
  those would need per-format decode elsewhere; out of scope for a container crate.
- `Muxer` buffers pushed PCM samples and writes the complete file on `finish()` —
  RIFF's `RIFF`/`data` chunk sizes must be known before the header is written, so
  (unlike `iso-bmff`'s fragmented fMP4 mux) there is **no incremental flush**. This is
  a real constraint of the format, not a corner cut: documented on `Muxer` itself.
- `parse()` is a single-shot function over a complete buffer (symmetric with
  `Muxer`), not an incremental `push_bytes`/`poll` demuxer — RIFF has no fragmentation
  concept to stream against, so an incremental API would be speculative surface with
  no real caller need (simplicity first).
- Unknown chunks (`LIST`, `fact`, …) are skipped by tag, matching `iso-bmff`'s
  `find_child_payload`-style tolerance of unrecognized boxes.

## Consequences

- No `mediaway-container` facade wiring yet (product-typed wrapper deferred, same
  as `ebml-webm` originally shipped demux-only before facade wiring existed).
- Compressed WAV payloads (ADPCM, µ-law, MP3-in-WAV) are rejected via
  `Error::UnsupportedFormatTag` rather than silently misinterpreted as PCM.

## References

- [`docs/spec/crate-packaging.md`](../../../docs/spec/crate-packaging.md) · ADR-0003
- ADR-0012 (workspace) — unprefixed freestanding-core naming
- [Microsoft `WAVEFORMATEX` reference](https://learn.microsoft.com/en-us/windows/win32/api/mmeapi/ns-mmeapi-waveformatex) (not pinned — implemented directly from the well-known fixed 16-byte PCM `fmt ` layout, no ambiguity requiring a cached copy)
