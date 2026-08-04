# Mediaway — Agent Guidelines

> **This file is the single source of truth (SSOT) for all coding agents** working in this repository — maintainers’ tools and **contributors’** assistants (Cursor, Claude Code, Copilot, etc.).
> Contributor AIs should start with [`docs/contributing/for-agents.md`](docs/contributing/for-agents.md), then this file.
> Tool entrypoints only point here — `CLAUDE.md` imports `@AGENTS.md`; Antigravity/Cursor use `.agents/rules/main.md`.
> **Edit rules only in this file.** Do not duplicate rules elsewhere.

Keep only always-needed conventions here. Details live under `docs/ai/`, `docs/conventions/`, `docs/spec/`, and `docs/adr/` — read them when relevant.

**Environment:** Rust Cargo workspace. MIT OR Apache-2.0 · no `libav*`/GPL in the Cargo graph · cross-platform (Windows / macOS / iOS / Android / Linux / Web). **Early development (pre-1.0) — not production-ready; APIs may change often** ([`docs/spec/status.md`](docs/spec/status.md)).

**Vision:** Zero-Copy paths (GPU handles **or** shared CPU buffers) / HW, sans-io cores, high→low first-class APIs, honest cost contracts — license/deps as a **boundary**, not the headline. Canonical: [`docs/spec/vision.md`](docs/spec/vision.md). **Sans-IO maximized** for mux/demux/bitstream/config: [`docs/spec/sans-io.md`](docs/spec/sans-io.md). **Low-level APIs first-class:** [`docs/spec/api-layers.md`](docs/spec/api-layers.md). **Zero-cost abstractions (ZCA):** [`docs/spec/zero-cost-abstractions.md`](docs/spec/zero-cost-abstractions.md) (ADR-0009). **Crate packaging:** [`docs/spec/crate-packaging.md`](docs/spec/crate-packaging.md) (ADR-0003 · naming v1 ADR-0012). **C-FFI:** [`docs/spec/c-ffi.md`](docs/spec/c-ffi.md) (ADR-0004). **GPU interop:** [`docs/spec/gpu-interop.md`](docs/spec/gpu-interop.md) (ADR-0005). **Caveats + code clarity:** [`docs/spec/caveats-and-clarity.md`](docs/spec/caveats-and-clarity.md) (ADR-0006). Zero-Copy marks: [`docs/ai/wiki/zero-copy/marks.md`](docs/ai/wiki/zero-copy/marks.md).

---

## Language policy

| Surface | Language |
|---------|----------|
| Agent ↔ user chat | **User's language** (e.g. Korean, Japanese, Spanish, etc.) |
| Repo artifacts | **English only** |
| **Everything else** | **English only** |

**English-only** includes: source comments, docs (`docs/**`, README, wiki), ADRs, specs, conventions, commit messages, PR titles/bodies, GitHub issues/comments, agent command/agent prompt files committed in-repo, and error messages intended for logs.

Do not mix non-English prose into repo artifacts. If an older file still has non-English prose, translate when you touch it.

---

## Rule 0 — Read the wiki before any work

**This is the highest-priority rule.**

For every request — code change, investigation, Q&A, planning — **read [`docs/ai/wiki/index.md`](docs/ai/wiki/index.md) first.** If related pages appear in the index, read those too.

- No exceptions. Even “trivial” tasks check the wiki first.
- Do not assume prior knowledge — the wiki may be newer than the code.
- Skipping this step is a rule violation.

After work, update the wiki with what you learned (see [#5 Wiki upkeep](#5-wiki-upkeep)).

Then, as needed by scope:

1. [`docs/spec/README.md`](docs/spec/README.md) — design SSOT
2. **Crate `docs/roadmap.md` + `adr/`** when working in that crate (crate root `README.md` is a short overview only — not an agent entrypoint)
3. [`docs/adr/README.md`](docs/adr/README.md) — **workspace-wide** decisions only
4. [`docs/conventions/`](docs/conventions/) — commits / [branches](docs/conventions/branches.md) / hooks / style / security / deps / testing / [docs-layout](docs/conventions/docs-layout.md)
5. [`docs/roadmap.md`](docs/roadmap.md) — platform order + index of crate roadmaps

**Do not** put agent rules, Rule 0, or wiki “read first” prose in root or crate `README.md` files. Those are for humans; agents use this file and `docs/ai/wiki/`. See [`docs/conventions/docs-layout.md`](docs/conventions/docs-layout.md) § Audience.

**Local scratch:** [`local/`](local/README.md) — gitignored personal/agent/machine notes and experiments. Not an SSOT; do not commit.

---

## Reference docs

- [stack.md](docs/ai/stack.md) — stack summary
- [tools.md](docs/ai/tools.md) — cargo / deny / lefthook / verification
- [layout.md](docs/ai/layout.md) — where things live
- **[wiki/index.md](docs/ai/wiki/index.md) — knowledge wiki; read first**
- [`docs/spec/vision.md`](docs/spec/vision.md) — product pillars
- [`docs/spec/status.md`](docs/spec/status.md) — maturity (not for production)
- [`docs/spec/maturity-bar.md`](docs/spec/maturity-bar.md) — earning correctness / stability / perf trust
- [`docs/conventions/benchmarking.md`](docs/conventions/benchmarking.md) — honest benches and baselines
- [`docs/benchmarks/machines.md`](docs/benchmarks/machines.md) — reference `machine_id` profiles
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — human contributor guide (not an agent entrypoint)
- [`docs/contributing/pull-requests.md`](docs/contributing/pull-requests.md) — PR author checklist
- [`docs/spec/sans-io.md`](docs/spec/sans-io.md) — sans-io for mux/demux/bitstream/config
- [`docs/spec/api-layers.md`](docs/spec/api-layers.md) — low-level APIs first-class
- [`docs/spec/zero-cost-abstractions.md`](docs/spec/zero-cost-abstractions.md) — ZCA design · minimize `Box` · SmallVec
- [`docs/conventions/error-handling.md`](docs/conventions/error-handling.md) — library errors via `thiserror`
- [`docs/spec/crate-packaging.md`](docs/spec/crate-packaging.md) — facade / backends / unprefixed cores (ADR-0003 · naming v1 ADR-0012)
- [`docs/spec/c-ffi.md`](docs/spec/c-ffi.md) — C ABI: per-capability `*-ffi` + optional umbrella features
- [`docs/spec/gpu-interop.md`](docs/spec/gpu-interop.md) — wgpu / WebGPU / Dawn-style adapters
- [`docs/spec/caveats-and-clarity.md`](docs/spec/caveats-and-clarity.md) — costly-path docs + code clarity
- [CLAUDE.md](CLAUDE.md) — Claude Code entry (`@AGENTS.md` only)

---

## Absolute rules (stop work if violated)

### License & legal

1. **No GPL / LGPL / AGPL / SSPL / BUSL deps** — including FFmpeg **crates** / linking `libav*`, x264, x265. Enforced by `cargo deny`. System `ffmpeg`/`ffprobe` binaries are OK as an **optional test/dev oracle** only ([`docs/adr/0002-system-oracle.md`](docs/adr/0002-system-oracle.md)); never required to build or run shipped Mediaway.

### Safety & code hygiene

1. **No secrets in commits** — `.env`, tokens, private keys
2. **No new `unwrap()` / `expect()` / `panic!()`** outside tests (`unwrap_used = deny`)
3. **`unsafe_code = deny` by default** — FFI/platform backends only: `#![allow(unsafe_code)]` + `// SAFETY:` (`forbid` cannot be overridden by `allow`)
4. **No bare TODOs** — `TODO(#issue)` only
5. **Source files ≤1000 lines** — staged `.rs` (and other source extensions) over 1000 lines are rejected by `forbid-long-source` pre-commit. Split modules instead.

### Process & docs

1. **Spec / design decisions require an ADR** — **crate-local** `adr/` for that crate’s backends/API; **`docs/adr/`** only for workspace-wide policy. See [`docs/conventions/docs-layout.md`](docs/conventions/docs-layout.md).
2. **Branch before new work** — before adding commits, check the current branch's purpose and freshness (`git status`, `git log -1`, and whether it's diverged from `origin/main`). If it's unrelated to the task at hand, already merged/landed upstream, or stale, create a fresh branch off `main` (not `origin/main`'s local `main` blindly — verify it's up to date first) instead of stacking new work onto it. One task's changes = one branch = one PR; do not let unrelated work accumulate on an existing branch. Naming and lifetime: [`docs/conventions/branches.md`](docs/conventions/branches.md).
3. **Direct push to `main`** — only trivial docs/typos + `.claude/*` tooling. Else PR
4. **If using `--no-verify`**, put `[skip-hooks: <reason>]` in the commit body
5. **English-only artifacts** — see [Language policy](#language-policy). Includes **commit messages and PR title/body** (and issues). Chat with the user may use their language. **Not enforced by git hooks** — format hooks stay mechanical; language is policy + review.
6. **Keep `local/` local** — machine-specific notes, agent scratch memory, experiments, bench raw outputs, and **downloaded external standards** belong under [`local/`](local/README.md) (gitignored). Do **not** commit them. Durable knowledge goes to the wiki / spec / ADR instead.
7. **External standards by URL + BLAKE3** — do not paste full ISO/ITU/MPEG/etc. text into wiki/spec. Record official URLs and pin file digests in [`docs/standards/registry.toml`](docs/standards/registry.toml); cache under `local/standards/` and **verify BLAKE3** on use (`bun tools/scripts/fetch-standard.ts`). When using **that script**, agents **must** pass `--ai-agent` so its `User-Agent` discloses an AI coding agent (humans omit the flag). **Do not** put a Mediaway / `Mediaway-standards-fetch` User-Agent on any other requests (browsers, curl, random fetches). Paywalled docs: lawful local copy only, then `pin`. See [`docs/conventions/external-standards.md`](docs/conventions/external-standards.md).

### Architecture & API shape

1. **Sans-IO for applicable cores** — mux, demux, bitstream transforms, timebase/interleave math, and CLI/config parsing must not open files/sockets/devices in the core. I/O lives in adapters only. See [`docs/spec/sans-io.md`](docs/spec/sans-io.md). Encoder/decoder/device backends are platform adapters (not sans-io).
2. **Low-level APIs stay public and usable** — traits, sans-io cores, packet/frame types, and `GpuBufferHandle` (and platform variants) must be first-class entry points. Convenience layers compose them; they must not be the only way to use a capability. See [`docs/spec/api-layers.md`](docs/spec/api-layers.md).
3. **Crate packaging** — facades (`mediaway-<capability>`), platform backends (`mediaway-<capability>-<platform>`), and **unprefixed** freestanding cores (`iso-bmff`, `iso-cenc`, …) stay separate. Thin Mediaway adapters over unprefixed cores live in the facade (e.g. `mediaway-container::mp4`), not as `mediaway-container-<format>` unless an ADR says otherwise. Do not fold WMF/WebCodecs into the facade as `cfg` modules, or I/O into sans-io cores. Naming v1: ADR-0012. See [`docs/spec/crate-packaging.md`](docs/spec/crate-packaging.md) · ADR-0003.
4. **C-FFI only in `*-ffi` crates** (optional umbrella `mediaway-ffi` with **minimal default features**). Do not add `extern "C"` to sans-io/platform cores. Prefer per-capability FFI over one fat link graph ([`docs/spec/c-ffi.md`](docs/spec/c-ffi.md) · ADR-0004). Rust remains the primary API.
5. **Streaming-first + async-capable** — prefer packet/frame/byte-chunk incremental APIs; whole-buffer only as convenience. Sans-io cores stay sync/poll (no mandatory async runtime). Async via facades/adapters with optional features. See [`docs/spec/async-and-streaming.md`](docs/spec/async-and-streaming.md) · ADR-0007.
6. **ZCA before code · minimize `Box`** — for non-trivial Rust, sketch zero-cost shape (enums / generics / typestate / ownership / alloc sites) in chat **before** implementing. Prefer those over hot-path `Box` / `dyn`. `smallvec` is approved for usually-small lists; add only at a justified call site. See [`docs/spec/zero-cost-abstractions.md`](docs/spec/zero-cost-abstractions.md) · ADR-0009. **ZCA ≠ Zero-Copy.**
7. **Library errors via `thiserror`** — public errors in library/sans-io/facade crates are `thiserror` enums with **English** `#[error]` messages (prefer `#[non_exhaustive]`). Do not use `anyhow` / `eyre` / `Box<dyn Error>` as the public library error type. See [`docs/conventions/error-handling.md`](docs/conventions/error-handling.md) · ADR-0010.
8. **Rust-idiomatic public APIs** — prefer `Type::open` / `Type::try_new` over C-style free functions (`open_video`, `auto::open`). Do **not** bake resolution/quality presets into product constructor names (`h264_1080p`, …); callers pass explicit size/codec. Details: [`docs/conventions/code-style.md`](docs/conventions/code-style.md) § Public Rust API shape.

### Performance & honesty

1. **Document costly paths** — any compat/fallback that copies across GPU APIs (e.g. OpenGL→D3D11), CPU readback, extra converts, stalls, or SW fallbacks must have rustdoc (+ catalog/ADR when cross-cutting) and honest names. No silent slow defaults. See [`docs/spec/caveats-and-clarity.md`](docs/spec/caveats-and-clarity.md) · ADR-0006.
2. **Code carries the contract** — public APIs must be understandable from source + rustdoc alone (ownership, errors, perf notes). External docs amplify; they must not be the only place a footgun is mentioned. Same ADR-0006.
3. **Honest benchmarks** — perf claims need methodology per [`docs/conventions/benchmarking.md`](docs/conventions/benchmarking.md). Label `zc` / `copy` / `readback` / `sw` / `pure` (`zc` = GPU **or** shared CPU; never present a copy/readback path as Zero-Copy). Published numbers need a [`machine_id`](docs/benchmarks/machines.md) (`ref-*` for official baselines). Comparable encode/decode/mux baselines include **system `ffmpeg` side-by-side** (`oracle_ref`) or an explicit N/A reason ([ADR-0002](docs/adr/0002-system-oracle.md)). Comparisons must be **fair**: steady/amortized timing (not cold CLI vs warm in-process as the headline); **same path class** for headlines (`zc` vs `zc`, not Mediaway Zero-Copy vs FFmpeg software as like-for-like).
4. **Mandatory `// clone:` on `.clone()`** — every `.clone()` / `.to_owned()` in non-test production code needs an adjacent English comment starting with `// clone:` explaining why borrow/move is impossible **or** what benefit the clone buys (share, ownership boundary, measured win). Exemptions: `tests/` and `*_tests.rs`; `Arc`/`Rc` refcount bumps may use `// clone: Arc share` (or `Rc share`). `#[derive(Clone)]` itself needs no comment. Prefer move/borrow, buffer reuse, and Zero-Copy handoffs (GPU handles **or** shared CPU buffers) over habitual clones, per-frame `Vec` churn, or silent `memcpy`. Prefer **vectorization-friendly** code over hand-written SIMD-by-default; name and document necessary copies. See [`docs/conventions/code-style.md`](docs/conventions/code-style.md) § Allocation, clone, and copy discipline.

### Testing & deps

1. **No committed test media binaries** — fixtures are **generated in Rust**, cached under `local/.cache/test-media/` (gitignored), and validated by **BLAKE3** against an expected digest (`ensure`). Do not add media/raw blobs to the repo for tests. See [`docs/conventions/testing.md`](docs/conventions/testing.md).
2. **New Cargo deps are deliberate** — do not add crates casually. Follow [`docs/conventions/deps-policy.md`](docs/conventions/deps-policy.md) and the high-perf catalog [`docs/conventions/perf-crates.md`](docs/conventions/perf-crates.md): prefer std/existing/local code; check license **and transitive** graph; maintenance; compile/size cost; alternatives; optional features; ADR when heavy. `cargo deny` must pass.
3. **Tiered tests · sibling units** — unit tests live in sibling `<basename>_tests.rs` (no inline `#[cfg(test)] mod tests { … }`). Integration in `tests/`; oracle/fuzz/bench/property per [`docs/conventions/testing.md`](docs/conventions/testing.md). Default suite must pass without system FFmpeg.

### Tooling

1. **Light repo scripts = Bun + TypeScript** — new maintainers’ utilities live under `tools/scripts/` and use Bun/TS. Do not add Node/Python/PowerShell script trees for shared tooling. Git hooks stay bash (`tools/hooks/`). Product CLIs stay Rust. See [`docs/conventions/scripts.md`](docs/conventions/scripts.md).

---

## Behavioral guidelines

Prefer care over speed. Apply judgment on trivial tasks.

### 1. Think before coding

- State assumptions. Ask when unsure.
- If multiple interpretations exist, present them — do not pick silently.
- Prefer simpler approaches; push back when useful.
- **For code changes:** before editing files, state the approach in **1–2 sentences in the user's language** and wait for confirmation. No formal plan docs / Proceed gates. Skip for obvious typos.
- **For non-trivial Rust:** also sketch the **ZCA shape** (types, ownership, typestate, where `Box`/`Vec`/`SmallVec` appear) before writing code — same chat confirmation. See Absolute rules § Architecture & API shape (ZCA).

### 2. Simplicity first

- No features beyond the request.
- No abstractions for one-off code.
- No error handling for impossible paths.
- Ask: “Would a senior call this over-engineered?” → simplify.

### 3. Surgical changes

- Do not “improve” adjacent code, comments, or formatting. Match existing style.
- **But** if your change makes the blast radius incoherent, restructure in place (no legacy dual paths).
- Fix warnings on files you touch (`-D warnings`).

### 4. Goal-driven execution

Turn work into verifiable goals. Multi-step: `[step] → verify: [check]` and loop.

### 5. Wiki upkeep

**Before work:** read [wiki/index.md](docs/ai/wiki/index.md). **After work:** update what you learned.

- New systems / features → add or update wiki pages. **Do not skip because “no page exists.”**
- Docs ≠ code → fix immediately. Removed behavior → delete wiki content.
- Indexes are navigation only — links + one-line summaries.
- **100-line limit per wiki file** — `check-wiki-size` hook rejects oversize writes. Split before exceeding.
- Timing / races / GPU fences / async platform callbacks → leave a mermaid `flowchart` (not prose-only). Separate actors with `subgraph` (e.g. render thread vs encode session vs WASM main).

Wiki pages are **English** (language policy).

### 6. Investigation workflow

1. Wiki → spec/ADR → code. Trace end-to-end. No guessing.
2. Confirm the flow with the user **in their language** — “Does this match?”
3. Record in the wiki (English).
4. Then implement.

### 7. No formal planning mode

Do not write `implementation_plan.md` or wait for Proceed. Exception: the 1–2 sentence direction check in rule 1 (chat only, user's language).

### 8. Commits

**Commit only when the user explicitly asks.** Format: [`docs/conventions/commits.md`](docs/conventions/commits.md) — Conventional Commits; **English** subject/body (and English PRs). Hook checks format only.
- Do not put AI agent names or `Co-Authored-By` in commits/PRs.

### 9. Token-efficient execution

Same quality, shorter path: read only what you need; avoid re-fetching known facts. Cut waste, not verification / tests / license gates.

### 10. Release notes

When you make release-note-worthy changes — user-visible features, fixes,
behavior changes, deprecations, breaking changes, or new platform/codec/
binding support — add a concise English bullet to the `## Unreleased` section
of [`RELEASE_NOTES.md`](RELEASE_NOTES.md) under the matching subsection
(`Added` / `Changed` / `Fixed` / `Removed` / `Deprecated` / `Breaking`). Skip
internal refactors, docs-only, test-only, and dev-tooling changes. At release,
finalize with `/release-notes <version>` (archive + reset flow): details in
[`docs/ai/wiki/meta/release-notes.md`](docs/ai/wiki/meta/release-notes.md).

---

## Rust / Mediaway specifics

### `unsafe`

- FFI crates/modules: `#![allow(unsafe_code)]` + `// SAFETY:` on every `unsafe`; document unsafe boundaries in the ADR for new backends. Full rule: Absolute rules § Safety & code hygiene.

### License

- Before new deps: [`docs/conventions/deps-policy.md`](docs/conventions/deps-policy.md) + [`security.md`](docs/conventions/security.md) + `cargo deny`. Full rule: Absolute rules § License & legal, § Testing & deps.

### Tests

```bash
cargo nextest run --workspace   # preferred when installed
cargo test --workspace          # fallback
cargo test --workspace --doc    # doctests
```

Layout and writing rules: [`docs/conventions/testing.md`](docs/conventions/testing.md) (sibling `*_tests.rs`, tiers, oracle). Optional: install system FFmpeg for oracle suites; default suite must pass without it.

### Benchmarks

When measuring or claiming performance: [`docs/conventions/benchmarking.md`](docs/conventions/benchmarking.md). Use path-class labels, [`machine_id`](docs/benchmarks/machines.md), **`oracle_ref`** when comparable, and **fair** timing (warmup / long jobs; isolate CLI spawn overhead).

### Hooks

| Gate | What |
|------|------|
| pre-commit | fmt, clippy, secrets, large-file, bare-TODO, .env |
| commit-msg | Conventional Commits **format** only |
| pre-push | clippy `--all-features`, tests, `cargo deny` |

Details: [`docs/conventions/hooks.md`](docs/conventions/hooks.md)

---

**Healthy signals:** small diffs, little redesign churn, clarifying questions *before* implementation, wiki that stays alive across sessions, chat in the user's language and artifacts in English.
