# ADR-0012: Crate naming v1 — unprefixed cores vs `mediaway-*`

- **Status**: Accepted
- **Date**: 2026-07-27
- **Deciders**: @dev-nyxie (+ agent)

## Context

Rust crates.io norms: reusable domain libraries use short **unprefixed** names; product/stack surfaces use a family prefix. Mediaway needs both freestanding cores and a product API surface.

Do not invent a freestanding crates.io name for a crate that still requires `mediaway-common` or Mediaway facades to compile.

## Decision

> **v1 naming:** Unprefixed names for libraries usable without Mediaway types. `mediaway-*` only for product-bound crates.

| Usable without Mediaway types/facades? | Naming |
|----------------------------------------|--------|
| Yes | Unprefixed domain name (`iso-bmff`, `iso-cenc`) |
| No (facade, OS backend, CLI, glue) | `mediaway-<name>` |

- **`mediaway-*`:** `mediaway-common`, facades (`mediaway-container`, `mediaway-encoder`, …), platform backends (`mediaway-<capability>-<platform>`), tools (`mediaway-avcli`, `mediaway-test-media`)
- **Unprefixed:** domain cores with no hard dependency on `mediaway-common` — currently `iso-bmff` (ISOBMFF/MP4 sans-io) and `iso-cenc` (ClearKey sample crypto)
- **Container layout:** MP4 in `iso-bmff`; product types in `mediaway-container` facade — no `mediaway-container-mp4`, no `mediaway-muxer`/`mediaway-demuxer` shims

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Always `mediaway-*` | Wrong signal for freestanding cores |
| `mediaway-container-mp4` as MP4 core | Couples ISOBMFF to Mediaway types; blocks reuse |

## Consequences

- Clear product vs reusable-core boundary; facade must convert `iso_bmff` ↔ `mediaway_common` types

## References

- [`docs/spec/crate-packaging.md`](../spec/crate-packaging.md), [`docs/conventions/repo-structure.md`](../conventions/repo-structure.md)
- ADR-0003
