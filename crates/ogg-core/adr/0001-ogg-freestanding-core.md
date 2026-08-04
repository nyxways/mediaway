# ADR-0001: `ogg` — freestanding Ogg page/packet mux + demux

- **Status**: Accepted
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `ogg`

## Context

Ogg (RFC 3533) is the native transport for Opus/Vorbis/FLAC. `iso-bmff`'s `Codec`
enum already has `Codec::Opus` for ISOBMFF muxing, but there was no native Ogg
transport anywhere in the workspace.

## Decision

> New unprefixed freestanding crate `ogg` (naming: ADR-0012), sans-io, no
> Mediaway dependency.

- **Mux is intentionally simple**: [`Muxer::push_packet`] writes exactly one page
  per packet — always spec-valid (correct CRC, correct lacing), but does not
  batch multiple small packets into one page the way a real encoder would, and
  rejects (`Error::PacketTooLargeForSinglePage`) packets over 65024 bytes rather
  than splitting them across continuation pages. A real bounded v1 scope, not a
  corner cut — every page produced is fully decodable by any conformant Ogg reader.
- **Demux is fully general**, because it must interoperate with real encoders'
  output regardless of what this crate's own mux produces: multiple packets per
  page, and packets spanning continuation pages, are both handled
  (`Demuxer::poll_packet`, tested against hand-built pages in
  `demux_tests.rs` that this crate's `Muxer` itself never emits).
- CRC-32 is a **from-scratch implementation of Ogg's specific non-reflected
  variant** (poly `0x04C11DB7`, MSB-first, init 0) — not the same algorithm as
  zlib/PNG `crc32` (bit-reflected). Implemented directly rather than pulling in a
  `crc`/`crc32fast` dependency, since the whole algorithm is ~10 lines and this
  crate otherwise has zero non-`bytes`/`thiserror` dependencies (deps-policy: new
  deps are deliberate, and a dependency-free from-scratch implementation of a
  15-line bit-by-bit CRC was judged simpler than adding a crate for it).
- `granule_position`/`bos`/`eos` are page-level fields, attached to the page
  that *completed* each packet (spec-precise for the last packet completed on
  a page). `page_index`/`page_count` record a packet's position among the
  packets completed on its finishing page, so a codec-aware layer (the
  `mediaway-container::ogg` facade) can back-compute each packet's own end
  position from the page granule — exact per-packet provenance without
  pulling codec knowledge into the transport core.
- Continuation-flag mismatches (a page's `continued` bit disagreeing with whether
  a partial packet is actually buffered) are a hard `Err`
  (`Error::ContinuationFlagMismatch`) — a real stream desync, never silently
  guessed past.

## Consequences

- No multi-packet-per-page batching or continuation-page splitting on the mux
  side — tracked in `docs/roadmap.md`, not silently dropped.
- No multi-logical-stream interleaving (one `Muxer` = one `serial`) — chained/
  multiplexed Ogg (e.g. Vorbis comments in a separate logical stream from audio)
  is out of v1 scope.
- No `mediaway-container` facade wiring yet (freestanding core only).

## References

- [RFC 3533](https://www.rfc-editor.org/rfc/rfc3533) (Ogg encapsulation format) — free IETF RFC, not pinned via the standards registry (short, unambiguous fixed-header + CRC spec; cross-checked instead via the mux↔demux round-trip tests, including hand-built multi-packet/continuation pages the crate's own mux never produces)
- ADR-0012 (workspace) — unprefixed freestanding-core naming
- `crates/adts/adr/0001-adts-freestanding-core.md`, `crates/mpeg-audio/adr/0001-mpeg-audio-freestanding-core.md` — same "frame already-encoded data, don't encode" boundary applied to sibling elementary-stream formats
