# ADR-0001: Workspace bootstrap

- **Status**: Accepted
- **Date**: 2026-07-27
- **Deciders**: @dev-nyxie (+ agent)

## Context

Mediaway is an FFmpeg-less, MIT/Apache-2.0 cross-platform AV infrastructure crate family starting from an empty repo. A greenfield monorepo needs consistent tooling, agent onboarding, license gates, and conventions before feature crates land.

Domain-specific rules from other stacks (Linux-only CI, simulation-first clippy bans, failpoint mandates) would conflict with media FFI and multi-platform work.

## Decision

> Bootstrap the Mediaway workspace with a standard Rust monorepo toolchain, permissive license enforcement, and agent-first documentation — tuned for cross-platform media, FFI `unsafe` exceptions, and **English-only artifacts / user-language chat**.

- Stable toolchain, rustfmt, workspace clippy lints
- `deny.toml` permissive allow-list + FFmpeg crate bans
- lefthook pre-commit / commit-msg / pre-push
- `docs/conventions/*`, ADR template, `AGENTS.md` SSOT + `docs/ai/wiki/` (Rule 0: wiki first)
- First crate scaffold: `mediaway-common`

Excludes Linux-only or simulation-first lints, nightly-only toolchains, and unrelated platform hook stacks. Optional gitleaks stays warn-skip if missing.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Code without toolchain | License/convention drift — license is product identity |
| Import foreign stack configs wholesale | Conflicts with cross-platform media FFI needs |
| Design all rules from scratch | Reinvents proven monorepo patterns unnecessarily |

## Consequences

- GPL/FFmpeg blocked from the first commit via deny/hooks; clear agent onboarding path
- Crate-scoped clippy / mandatory nextest can be added later if needed

## References

- `docs/conventions/`
- `docs/ai/wiki/`
