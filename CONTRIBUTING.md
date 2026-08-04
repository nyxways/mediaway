# Contributing to Mediaway

Thanks for helping build Mediaway. This project is **early development** (`0.x`) and **not recommended for production** yet — see [docs/spec/status.md](docs/spec/status.md). Contributions are still welcome: design feedback, docs, tests, and carefully scoped code.

**By contributing, you agree that your contributions are licensed under the same dual license as the project: MIT OR Apache-2.0** ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)).

## Quick start

```bash
git clone https://github.com/nyxways/mediaway.git
cd mediaway
rustup show                          # toolchain from rust-toolchain.toml
cargo install lefthook cargo-deny
lefthook install
cargo test --workspace
```

Optional: `cargo-nextest`, `gitleaks`, and a system `ffmpeg` / `ffprobe` for oracle tests ([docs/conventions/testing.md](docs/conventions/testing.md)).

Machine-local scratch (gitignored): [local/](local/README.md) — personal/agent notes, experiments, HW facts. Do not commit.

## Using an AI coding assistant

If you use Cursor, Claude Code, Copilot, or similar, **point the assistant at the agent brief first**:

1. [docs/contributing/for-agents.md](docs/contributing/for-agents.md) — what contributor AIs must read
2. Root [AGENTS.md](AGENTS.md) — full rules SSOT (also auto-loaded by many tools via `CLAUDE.md` / `.agents/`)

Ask your AI to follow those before editing. Chat may be in your language; **all commits, PRs, and repo docs must stay English**.

## Before you change design

1. Read [docs/spec/vision.md](docs/spec/vision.md) and [docs/spec/status.md](docs/spec/status.md).
2. Skim packaging / sans-io / API layer rules:
  - [docs/spec/crate-packaging.md](docs/spec/crate-packaging.md)
  - [docs/spec/sans-io.md](docs/spec/sans-io.md)
  - [docs/spec/api-layers.md](docs/spec/api-layers.md)
3. Non-trivial design → write or update an **ADR** (crate-local `adr/` by default; workspace `docs/adr/` only for cross-cutting policy).
4. Prefer a short issue or discussion before large refactors.



## How we work


| Topic         | Rule                                                                                                                                                            |
| ------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Language      | **English** for commits, PRs, issues, docs, and code comments                                                                                                   |
| Branches      | Trunk-based: short `feat/…`, `fix/…`, … — [docs/conventions/branches.md](docs/conventions/branches.md)                                                        |
| Commits       | [Conventional Commits](https://www.conventionalcommits.org/) — [docs/conventions/commits.md](docs/conventions/commits.md)                                     |
| PRs           | One focused change; squash merge to `main`; do not push straight to `main` for non-trivial work                                                                 |
| Hooks         | Install lefthook; keep pre-commit / pre-push green — [docs/conventions/hooks.md](docs/conventions/hooks.md)                                                   |
| Issues        | Bugs, questions, and most requests via [GitHub Issues](https://github.com/nyxways/mediaway/issues) — [docs/conventions/issues.md](docs/conventions/issues.md) |
| Light scripts | Bun + TypeScript under `tools/scripts/` — [docs/conventions/scripts.md](docs/conventions/scripts.md)                                                          |
| License       | No GPL/LGPL/FFmpeg **crates** or linking `libav`* — [docs/conventions/security.md](docs/conventions/security.md)                                              |
| Tests         | Prefer Rust-generated fixtures; system FFmpeg is an optional oracle only                                                                                        |




## What to contribute

**Good fits right now**

- Spec / ADR clarity, roadmap checkboxes, examples of API shapes
- `mediaway-common` types, sans-io mux/demux design and tests
- Windows-first backend spikes in `mediaway-*-windows` (when started)
- Test helpers, oracle harnesses, CI ideas

**Please avoid**

- Pulling FFmpeg or GPL codecs into the Cargo graph
- Mega-PRs that mix unrelated crates
- Committing media binaries (fixtures are generated + BLAKE3-verified)
- Claiming production readiness in docs or release notes



## Documentation

Human-oriented map: [docs/contributing/](docs/contributing/README.md).


| Area                                           | Where                                       |
| ---------------------------------------------- | ------------------------------------------- |
| Vision / design                                | `docs/spec/`                                |
| Process (commits, hooks, style, deps, testing) | `docs/conventions/`                         |
| Workspace decisions                            | `docs/adr/`                                 |
| Per-crate overview / plan / decisions          | `crates/<name>/README.md`, `crates/<name>/docs/roadmap.md`, `crates/<name>/adr/` |
| Platform order                                 | [docs/roadmap.md](docs/roadmap.md)        |


Write docs in **English**. Keep root/`README.md` for humans (setup, status, links) — not agent runbooks.

## Code expectations

- Match existing style and workspace Clippy lints ([docs/conventions/code-style.md](docs/conventions/code-style.md)).
- `unsafe` only in platform/FFI crates with `#![allow(unsafe_code)]` and `// SAFETY:` on every block.
- Sans-IO cores stay free of file/socket/GPU session I/O; OS backends live in `mediaway-<capability>-<platform>` crates.
- Low-level APIs stay public and usable; convenience layers compose them ([docs/spec/api-layers.md](docs/spec/api-layers.md)).
- Costly compat paths (GL→DX copy, CPU readback, …) need honest names + rustdoc; code should carry the contract ([docs/spec/caveats-and-clarity.md](docs/spec/caveats-and-clarity.md)).
- Hot paths: careful with clone / allocation / copy ([docs/conventions/code-style.md](docs/conventions/code-style.md)).
- Keep source files ≤1000 lines (pre-commit enforced).
- Light maintainers’ scripts: Bun + TypeScript (`tools/scripts/`) — [scripts.md](docs/conventions/scripts.md).
- Streaming-first + async-capable (no Tokio-in-core by default) — [async-and-streaming.md](docs/spec/async-and-streaming.md).
- Issues: use GitHub forms for bugs/questions/features — [issues.md](docs/conventions/issues.md).
- New dependencies: deliberate review ([docs/conventions/deps-policy.md](docs/conventions/deps-policy.md)) — not convenience adds.
- Perf claims / hot paths: follow [docs/conventions/benchmarking.md](docs/conventions/benchmarking.md).



## Pull request checklist

Canonical full list: [docs/contributing/pull-requests.md](docs/contributing/pull-requests.md) (also pre-filled via `.github/PULL_REQUEST_TEMPLATE.md`).

Highlights:

- [ ] English Conventional Commits + English PR
- [ ] fmt / clippy / tests / deny (as needed) green
- [ ] **Docs sync:** rustdoc, roadmap, ADR/spec, caveat catalog, wiki as applicable
- [ ] Costly paths documented; public API rustdoc complete
- [ ] New Cargo deps justified per [deps-policy.md](docs/conventions/deps-policy.md)
- [ ] No secrets, no test-media binaries, no GPL/FFmpeg crates
- [ ] No `local/` scratch staged



## Questions

**Please use [GitHub Issues](https://github.com/nyxways/mediaway/issues/new/choose)** for questions and general inquiries — that is the supported channel for [nyxways/mediaway](https://github.com/nyxways/mediaway).

For architecture direction, link the relevant `docs/spec/` or ADR page so review stays grounded. Long brainstorming may also use Discussions; trackable work still belongs in an Issue. Security reports: see [docs/conventions/security.md](docs/conventions/security.md) (not a public issue).

## Conduct

Be respectful and constructive. Assume good faith; disagree on designs with concrete alternatives. Harassment or hostile behavior is not acceptable.