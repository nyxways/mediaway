# For contributor AI assistants

**Audience:** coding agents helping a **human contributor** to Mediaway (Cursor, Claude Code, Copilot Chat, etc.).

You are not exempt from project rules. Root [`AGENTS.md`](../../AGENTS.md) is the **rules SSOT** for every agent in this repo — including yours. This page is the **contributor onboarding brief**: what to load first and how to behave on a contribution.

## Mandatory load order

1. This file  
2. [`AGENTS.md`](../../AGENTS.md) — absolute rules (license, unsafe, sans-io, packaging, language, …)  
3. [`docs/ai/wiki/index.md`](../ai/wiki/index.md) — then any related wiki pages (**Rule 0**)  
4. [`docs/spec/status.md`](../spec/status.md) — early development; **not for production**  
5. [`docs/spec/vision.md`](../spec/vision.md)  
6. Task-relevant: [`crate-packaging.md`](../spec/crate-packaging.md) · [`sans-io.md`](../spec/sans-io.md) · [`api-layers.md`](../spec/api-layers.md)  
7. Human process: [`CONTRIBUTING.md`](../../CONTRIBUTING.md) · [`pull-requests.md`](pull-requests.md) · crate `docs/roadmap.md` + `adr/` for the crate you touch  

Do **not** treat root `README.md` as your rulebook. Prefer this file + `AGENTS.md` + wiki + spec.

## Language

| Surface | Language |
|---------|----------|
| Chat with the contributing human | **Their** language |
| Commits, PRs, issues, docs, comments, ADRs | **English only** |

## Hard constraints (summary)

Full text in `AGENTS.md`. Do not violate these:

- No GPL/LGPL/FFmpeg **Cargo** deps or linking `libav*` (`cargo deny` / ADR-0002)
- System `ffmpeg`/`ffprobe` OK as **optional test oracle** only; default tests must pass without them
- Sans-IO cores stay I/O-free; OS backends are `mediaway-<capability>-<platform>` crates (ADR-0003)
- Low-level APIs stay public; no convenience-only black boxes
- C-FFI only via future `mediaway-*-ffi` / optional feature-gated `mediaway-ffi` (Rust APIs first; no `extern "C"` in cores)
- Costly paths (copies, readback, SW fallback) need honest names + rustdoc; code carries the contract (ADR-0006)
- Hot paths: no casual `.clone()` / alloc churn / silent memcpy ([`code-style.md`](../conventions/code-style.md))
- Source files ≤1000 lines (pre-commit `forbid-long-source`)
- Light repo scripts: Bun + TypeScript (`tools/scripts/`) — not Node/Python trees ([`scripts.md`](../conventions/scripts.md))
- Streaming-first; async without mandatory runtime in sans-io cores ([`async-and-streaming.md`](../spec/async-and-streaming.md))
- External standards: URL + notes in-repo; full text only under `local/standards/` with **BLAKE3** pin ([`external-standards.md`](../conventions/external-standards.md))
- New Cargo deps only after [`deps-policy.md`](../conventions/deps-policy.md) review (not casual adds)
- Benchmarks: [`benchmarking.md`](../conventions/benchmarking.md) — path labels, `machine_id`, fair `oracle_ref` (CLI overhead accounted for)
- No committed test media binaries; use `mediaway-test-media` + BLAKE3
- No new `unwrap`/`expect`/`panic` outside tests; `unsafe` only in platform crates with `// SAFETY:`
- Design changes need an ADR; keep roadmaps in sync
- Do not claim production readiness

## Contribution behavior

- Prefer small, reviewable diffs aligned with [`docs/roadmap.md`](../roadmap.md) (Windows → Web → Linux → other)
- Before large design work: propose options, then ADR — do not silently invent a new architecture
- Update English docs/ADR/roadmap when behavior or public API shape changes
- Leave agent runbooks out of `README.md` / `CONTRIBUTING.md` (humans own those; you own wiki/`AGENTS.md` updates when you learn something durable)
- Use [`local/`](../../local/README.md) for machine-specific or session scratch (gitignored); promote durable notes to the wiki
- After non-trivial investigation: update `docs/ai/wiki/` (≤100 lines/file, English)

## What “done” looks like for a PR

- Full author checklist: [`pull-requests.md`](pull-requests.md)
- Hooks mindset: fmt, clippy `-D warnings`, tests, `cargo deny`
- Conventional Commits + PR text in English ([`commits.md`](../conventions/commits.md))
- Docs sync (rustdoc / roadmap / ADR / caveats / wiki) in the **same** change when code warrants it

## If unsure

Stop and ask the human. Point them at the relevant spec/ADR instead of guessing license or packaging.
