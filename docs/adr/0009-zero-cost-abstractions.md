# ADR-0009: Zero-cost abstractions first · minimize `Box` · SmallVec when bounded

- **Status**: Accepted
- **Date**: 2026-07-27
- **Deciders**: @dev-nyxie (+ agent)

## Context

Mediaway's performance story needs **Rust zero-cost abstractions** (enums, generics, typestate) — not only GPU Zero-Copy. `Box`/`dyn` on ISOBMFF paths fights inlining and forces heap shapes ZCA would avoid.

Many hot structures are small and bounded (NAL lists, track tables). Agents need a workspace rule: plan ZCA shape before coding. **ZCA** = types compile away; **Zero-Copy** = avoid data copies (GPU **or** shared CPU) — do not conflate.

## Decision

> **(1)** Non-trivial Rust work plans ZCA-shaped design **before** implementation. **(2)** Prefer enums, generics, typestate over `Box`/`dyn` on hot and sans-io paths. **(3)** `smallvec` is approved for usually-small inline vectors — add at first justified call site only.

- **Hot/sans-io cores:** no `Box`/`dyn` unless ADR or rustdoc states why (FFI boundary, rare cold path)
- **`smallvec`:** when `N` is usually small (≤8/≤16); pin via `[workspace.dependencies]`; first site: `iso-bmff` track tables (`INLINE_TRACKS = 4`), fragment rows (`INLINE_SAMPLES = 32`)
- Canonical detail: [`zero-cost-abstractions.md`](../spec/zero-cost-abstractions.md)

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Allow `Box<dyn …>` freely in cores | Defeats ZCA / inlining goals |
| Mandate `arrayvec` only | No heap spill when overflow is real |

## Consequences

- Agents design types before code; first `smallvec` add still needs deny/PR checklist

## References

- [`docs/spec/zero-cost-abstractions.md`](../spec/zero-cost-abstractions.md)
- [`docs/conventions/code-style.md`](../conventions/code-style.md), [`deps-policy.md`](../conventions/deps-policy.md)
