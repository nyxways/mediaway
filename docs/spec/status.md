# Project status

**Mediaway is early development (pre-1.0).** **Production use is not recommended.**

## API stability (pre-1.0)

Until **1.0**, there is **no stable API promise**:

- Public types, traits, module layout, and function signatures may change **without a deprecation cycle**
- Crate boundaries and feature flags may be split, renamed, or removed
- Backend behavior and encode/decode path selection may change as platforms and Auto/preference APIs land
- Dependents should pin git revisions (or equivalent) and expect breakage on pull

Semantic versioning for crates still applies within `0.x` (breaking changes bump the minor or are called out in the changelog when published). Stability for production begins only after an explicit **1.0** (and matching notes here).

| Signal | Meaning |
|--------|---------|
| Version | `0.x` — breaking changes without a stable promise |
| crates.io | `publish = false` until maturity is revised here |
| Completeness | Scaffolds and design docs first; many crates have no public API yet |
| Support | Best-effort while the stack is being built |
| When to reconsider | After documented milestones **and** evidence against the [maturity bar](maturity-bar.md) (corpora, benches, soak, API stability for claimed scopes) — then an explicit note here |

Suitable today: experimentation, design feedback, contributing, early integration spikes that tolerate breakage.

Not suitable yet: shipping end-user products that depend on Mediaway for encode/decode/mux in production.

**Long game:** earn trust on correctness, stability, and performance for the scopes we claim — see [`maturity-bar.md`](maturity-bar.md).

How to contribute: [`CONTRIBUTING.md`](../../CONTRIBUTING.md) · [`docs/contributing/`](../contributing/).

Related: [`vision.md`](vision.md) · [`overview.md`](overview.md) · [`docs/roadmap.md`](../roadmap.md).
