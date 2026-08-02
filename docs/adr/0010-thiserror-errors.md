# ADR-0010: Library errors via `thiserror`

- **Status**: Accepted
- **Date**: 2026-07-27
- **Deciders**: @dev-nyxie (+ agent)

## Context

Workspace pins `thiserror`; `iso-bmff` and `mediaway-test-media` already use it. Without a workspace rule, crates will diverge (`String` errors, `anyhow` in libraries).

Library callers and C-FFI need **stable, typed, English** errors — not opaque trait objects.

## Decision

> **Library and sans-io crates** expose public errors as **`thiserror` enums**. Do **not** use `anyhow` / `eyre` / `Box<dyn std::error::Error>` as the public library error type.

- One primary error enum per crate, English `#[error("…")]` messages; prefer structured variants + `#[from]`
- Facades may re-export under role alias (`MuxError`); no second hierarchy
- C ABI maps typed errors to codes later — Rust enums remain source of truth

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| `anyhow` in every crate | Erases variants; bad for libraries / FFI |
| Single workspace `MediawayError` mega-enum | Couples unrelated crates; fights packaging |

## Consequences

- Uniform reviewable errors; use `#[non_exhaustive]` on public library errors as features grow

## References

- [`docs/conventions/error-handling.md`](../conventions/error-handling.md)
