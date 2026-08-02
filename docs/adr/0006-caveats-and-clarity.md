# ADR-0006: Perf/compat caveats honesty + code as primary documentation

- **Status**: Accepted
- **Date**: 2026-07-27
- **Deciders**: @dev-nyxie (+ agent)

## Context

Mediaway offers Zero-Copy paths (GPU-resident **or** shared CPU) and HW sessions alongside escape hatches (CPU readback, payload memcpy, cross-API texture copies, format conversions). Silent trade-offs cause developers to ship slow pipelines — violating **No performance surrender**.

Callers and maintainers often read only code. APIs opaque without source-level contract fail the first-class low-level API goal.

## Decision

> **(1)** Any option that can **degrade performance**, **copy** between GPU APIs, **read back to CPU**, or surprise perf-sensitive users **must** be documented and named so cost is obvious.  
> **(2)** **Code alone should suffice** to use and maintain a surface — rustdoc, names, types, and `SAFETY` carry the contract; external docs amplify, not replace.

- Document cross-API copies, CPU round-trips, blocking sync, SW fallbacks, lossy interop in rustdoc; prefer names like `copy_…`, `readback_…`, `compat_…`
- Crate ADR/spec for non-obvious cross-cutting cases; silent slow defaults **forbidden** unless only option and documented at choice site
- Public items: meaningful rustdoc (what/when/ownership/errors/perf); explicit enums over boolean soup

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Docs-only warnings, cute API names | Call sites look cheap; people skip docs |
| Hidden auto-fallback to copy paths | Violates performance pillar |

## Consequences

- Honest APIs with clear review bar; slightly more verbose names and rustdoc

## References

- [`docs/spec/caveats-and-clarity.md`](../spec/caveats-and-clarity.md), [`docs/spec/gpu-interop.md`](../spec/gpu-interop.md)
- [`docs/conventions/code-style.md`](../conventions/code-style.md)
