# Getting started (contributors)

## Prerequisites

- Rust **stable** matching [`rust-toolchain.toml`](../../rust-toolchain.toml) (MSRV **1.85**, edition **2024**)
- Git
- On Windows: a bash-capable environment for lefthook scripts (Git Bash is fine)
- Optional: [Bun](https://bun.sh) when working on `tools/scripts/` ([`scripts.md`](../conventions/scripts.md))

## Setup

```bash
git clone https://github.com/nyxways/mediaway.git
cd mediaway
rustup show
cargo install lefthook cargo-deny
lefthook install
```

Optional:

```bash
cargo install cargo-nextest
# gitleaks: scoop install gitleaks  |  brew install gitleaks
# oracle tests: install ffmpeg / ffprobe on PATH
```

## Verify

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check advisories licenses bans sources
```

Or rely on lefthook `pre-commit` / `pre-push` after `lefthook install`.

## First useful reads

1. [`docs/spec/status.md`](../spec/status.md) — maturity
2. [`docs/spec/vision.md`](../spec/vision.md) — goals
3. [`docs/roadmap.md`](../roadmap.md) — platform order + crate index
4. Root [`CONTRIBUTING.md`](../../CONTRIBUTING.md) — full rules

Then open the crate you care about: `crates/<name>/docs/roadmap.md`.

## AI assistants

Point your coding agent at [`for-agents.md`](for-agents.md) (then root `AGENTS.md`).
