# Status & Stability

**Mediaway is early development (pre-1.0). Production use is not recommended.**

## No stable API promise yet

Until 1.0:

- Public types, traits, module layout, and function signatures may change
  **without a deprecation cycle**.
- Crate boundaries and feature flags may be split, renamed, or removed.
- Backend behavior and encode/decode path selection may change as platforms
  and `Auto`/preference APIs land.

If you depend on Mediaway today, pin a git revision and expect breakage on
pull — `publish = false` on crates.io until this changes.

| Signal | Meaning |
|--------|---------|
| Version | `0.x` — breaking changes without a stable promise |
| Completeness | Scaffolds and design docs first; many crates have no public API yet |
| Support | Best-effort while the stack is being built |

## Suitable today

Experimentation, design feedback, contributing, early integration spikes
that can tolerate breakage.

## Not suitable yet

Shipping end-user products that depend on Mediaway for encode/decode/mux in
production.

## Checking what's actually implemented

Status tables (this book's [Reference](../reference/codec-support.md)
section, and the project README) distinguish first-class (✅), Zero-Copy
(⚡), best-effort/prototype (🆗), planned (🛠️), genuinely blocked (❌), and
out-of-scope-here (👻) per cell — read the legend, not just the marks, since
"planned" and "blocked" mean very different things for whether to wait or
look elsewhere.

Full detail: [`docs/spec/status.md`](https://github.com/nyxways/mediaway/blob/main/docs/spec/status.md) ·
[`docs/spec/maturity-bar.md`](https://github.com/nyxways/mediaway/blob/main/docs/spec/maturity-bar.md)
(what it takes to earn correctness/stability/performance trust for a given
scope) in the repository.
