# Pull requests

## Branch

Create a short-lived branch from `main` ([`docs/conventions/branches.md`](../conventions/branches.md)):

```text
feat/muxer-mp4-header
fix/common-rational-reduce
docs/contributing-guide
```

## Commits

English [Conventional Commits](https://www.conventionalcommits.org/) — [`docs/conventions/commits.md`](../conventions/commits.md).

```text
feat(muxer): add sans-io MP4 header builder

Explain why in the body when the change is non-obvious.
```

## PR description (English)

Use the GitHub template (`.github/PULL_REQUEST_TEMPLATE.md`) or fill Objective / Solution / Testing equivalently.

Issues vs feature discussions: [`../conventions/issues.md`](../conventions/issues.md).

---

## Author checklist

Tick every item that applies. **N/A** is fine when truly irrelevant — do not skip silently if unsure.

### Quality gates

- [ ] `cargo fmt` clean
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` (or crate-focused equivalent) clean
- [ ] Tests added/updated and passing (`cargo test` / nextest); default suite does **not** require system FFmpeg
- [ ] CI green on the PR (`.github/workflows/ci.yml`) when remote is used
- [ ] Perf-sensitive changes: benches per [`benchmarking.md`](../conventions/benchmarking.md) with **`machine_id`**, fair **`oracle_ref`** (steady/amortized; CLI overhead not dumped on FFmpeg only), **or** deferral + issue; no fake Zero-Copy; official baselines from `ref-*` only
- [ ] `cargo deny check advisories licenses bans sources` clean if deps changed (recommended always before push)
- [ ] lefthook hooks green locally (or failure explained in the PR)

### Language & commits

- [ ] Commit messages: English Conventional Commits
- [ ] PR title and body: **English**
- [ ] No bare `TODO` / `FIXME` (use `TODO(#issue)`)

### Documentation sync (when code or design changes)

Update **all that apply** in the same PR (or explain deferral with an issue link):

- [ ] **rustdoc** on new/changed public items (purpose, ownership/lifetime, errors, perf/compat notes)
- [ ] Crate `docs/roadmap.md` if a stage checkbox moved
- [ ] Crate `adr/` if backend/API/behavior decision changed (or workspace `docs/adr/` if cross-cutting)
- [ ] `docs/spec/` if a workspace design rule changed
- [ ] [`docs/spec/caveats-and-clarity.md`](../spec/caveats-and-clarity.md) catalog (or linked crate notes) if a costly/compat path was added
- [ ] Workspace [`docs/roadmap.md`](../roadmap.md) / wiki crate-map if workspace members or packaging changed
- [ ] `docs/ai/wiki/` short pointer update when you learned something durable (agents; humans welcome too)
- [ ] Root `README.md` / `CONTRIBUTING.md` only if human-facing setup/status links changed — **not** for agent runbooks

### Architecture & API

- [ ] Sans-IO cores stay free of file/socket/device I/O ([`sans-io.md`](../spec/sans-io.md))
- [ ] Platform work in `mediaway-*-<platform>` (or planned); no mega-`cfg` dump in facades ([`crate-packaging.md`](../spec/crate-packaging.md))
- [ ] Low-level surfaces stay public; no convenience-only black box ([`api-layers.md`](../spec/api-layers.md))
- [ ] `extern "C"` only in `*-ffi` crates ([`c-ffi.md`](../spec/c-ffi.md))
- [ ] Costly paths use honest names (`copy_…`, `readback_…`, …); no silent slow default ([`caveats-and-clarity.md`](../spec/caveats-and-clarity.md))
- [ ] Hot paths avoid casual `.clone()` / per-frame alloc / silent copies ([`code-style.md`](../conventions/code-style.md))
- [ ] No staged source file **>1000 lines** (split modules; `forbid-long-source`)

### Safety, license, fixtures, dependencies

- [ ] No secrets (`.env`, keys, tokens)
- [ ] No GPL/LGPL/FFmpeg **crates** or `libav*` link; system ffmpeg only as optional test oracle
- [ ] No committed test media binaries (Rust generators + BLAKE3 cache only)
- [ ] No `local/` scratch (agent/machine/experiments) staged — only `local/README.md` / `local/.gitignore` may be tracked
- [ ] **New Cargo deps:** reviewed per [`deps-policy.md`](../conventions/deps-policy.md) (need, transitive license, maintenance, cost, alternatives); justified in the PR; ADR if heavy/codecs/FFI; `default-features = false` when useful; `cargo deny` clean
- [ ] New `unsafe`: `#![allow(unsafe_code)]` at the right crate + `// SAFETY:` on every block; ADR notes the boundary

### Scope

- [ ] PR stays focused (split unrelated work)
- [ ] Blast-radius cleanup: no leftover dual paths / dead shims inside touched code
- [ ] Does **not** claim production readiness ([`status.md`](../spec/status.md))

---

## Reviewer bar

Reviewers (human or `mediaway-reviewer`) treat gaps in the applicable checklist as **Blocking** when they match absolute rules — especially license, **casual new deps**, unsafe, caveats/rustdoc, and doc sync for API/behavior changes.

## Merge

Prefer **squash merge** into `main`. Do not force-push `main`.
