# Contributing

Mediaway takes contributions — code, design feedback, and bug reports.

- Human contributor guide:
  [`CONTRIBUTING.md`](https://github.com/nyxways/mediaway/blob/main/CONTRIBUTING.md)
- Getting-started, docs map, PR process:
  [`docs/contributing/`](https://github.com/nyxways/mediaway/tree/main/docs/contributing)
- PR author checklist (doc sync, quality gates):
  [`docs/contributing/pull-requests.md`](https://github.com/nyxways/mediaway/blob/main/docs/contributing/pull-requests.md)
- Bug/crash/docs issues use the issue tracker; feature ideas start as
  [GitHub Discussions](https://github.com/nyxways/mediaway/discussions) —
  see [`docs/contributing/issues.md`](https://github.com/nyxways/mediaway/blob/main/docs/contributing/issues.md)
  for the split.

## Dev setup

```bash
# Toolchain (rust-toolchain.toml pins stable)
rustup show

# Hooks
cargo install lefthook cargo-deny
lefthook install

# Optional — some suites use these as test/dev oracles or accelerate nextest
cargo install cargo-nextest gitleaks
```

```bash
cargo nextest run --workspace   # preferred when installed
cargo test --workspace          # fallback
```

## If you're bringing an AI coding assistant

Start with
[`docs/contributing/for-agents.md`](https://github.com/nyxways/mediaway/blob/main/docs/contributing/for-agents.md),
then the project's `AGENTS.md` (single source of truth for repo conventions
— license/safety rules, architecture rules, commit format, and more).
