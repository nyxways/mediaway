# ADR-0008: No narrow-vertical mandate

- **Status**: Accepted
- **Date**: 2026-07-27
- **Deciders**: @dev-nyxie (+ agent)

## Context

Mediaway may target broad format and codec surfaces (e.g. full MP4 parity, multiple codecs across platforms). Workspace policy must not cap ambition by mandating "ship narrow verticals before breadth."

Maturity still requires evidence for whatever we claim — corpora, oracles, benches, soak, honest docs — before production readiness.

## Decision

> **No narrow-vertical / ship-narrow-before-breadth mandate.** Breadth of claimed formats and codecs is allowed; maturity still requires evidence for whatever we claim.

- Roadmaps and ADRs may target wide surfaces without policy conflict
- Agents must not block work solely because a scope is "too wide"
- Do not flip [`status.md`](../spec/status.md) to production without [`maturity-bar.md`](../spec/maturity-bar.md) evidence for claimed scopes
- Evidence burden scales with claim size — large claims need large corpora/tests

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Keep narrow-vertical as soft guidance | Caps ambition without improving quality |
| Drop all maturity evidence requirements | Still need correctness/stability/perf proof before production |

## Consequences

- Wide roadmap goals are policy-valid; evidence requirements remain the production gate

## References

- [`docs/spec/maturity-bar.md`](../spec/maturity-bar.md), [`docs/spec/status.md`](../spec/status.md)
- [`docs/spec/vision.md`](../spec/vision.md)
