# ADR-0001: Paired sans-io ISOBMFF container (ZCA)

- **Status**: Accepted
- **Date**: 2026-07-27
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `iso-bmff`

## Context

Separate `mediaway-muxer` / `mediaway-demuxer` duplicated ISOBMFF layout and fought zero-cost sharing. Callers own I/O; cores must stay pure sans-io. `Box`/`dyn` are forbidden on this path. Product types stay out of this crate (ADR-0012).

## Decision

> One unprefixed crate `iso-bmff` owns mux + demux as a pair, sharing an ISOBMFF layer built with enums, generics, and typestate — no `Box`. Mediaway-typed APIs live in `mediaway-container::mp4`.

1. **Own types** — `Track` / `Sample` / `Codec` / `Rational` (no `mediaway-common`).
2. **Typestate muxer** — `Muxer<Open>` (tracks) → `begin()` → `Muxer<Live>` (samples).
3. **Shared `isobmff`** — `FourCc`, header parse/write, `ByteSource`, `Vec<u8>` via `write_box`.
4. **fMP4 primary** — `ftyp`/`moov`/`moof`/`mdat`; Stage 1 codecs H.264 + AAC.
5. **No file adapters** — callers supply/consume bytes.
6. **Facade** — `mediaway-container` maps to Mediaway types + `Mux`/`Demux` traits; apps depend on the facade (or on `iso-bmff` directly).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| `mediaway-container-mp4` as the core | Couples ISOBMFF to Mediaway types |
| Extra `mediaway-muxer` / `mediaway-demuxer` shims | Useless indirection; apps use the facade |
| Feature-split mux/demux | Shared boxes; little link win |
| `Box<dyn IsoBox>` | Violates ZCA / alloc rules |
| std-io in-crate | I/O is caller responsibility |

## Consequences

### Positive

- One layout truth; freestanding reuse; inlinable hot paths

### Negative / Trade-offs

- Facade conversion layer for Mediaway callers
