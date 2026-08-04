# Git hooks

Local lefthook gates plus **GitHub Actions** (`.github/workflows/ci.yml`). Dev-machine hooks stay the fast loop; CI is the merge gate.

## Install

```bash
cargo install lefthook cargo-deny
lefthook install
# optional
cargo install cargo-nextest
# gitleaks: scoop install gitleaks  |  brew install gitleaks
```

## Gates

### pre-commit

- `cargo fmt` auto-apply + restage
- `cargo clippy --fix` + `-D warnings` — **scoped** to the staged crates' affected
  closure ([`ci-affected.ts`](../../tools/scripts/ci-affected.ts), dependency-tree
  reachability: one crate lints itself + its transitive dependents; NONE skips)
- secrets (`gitleaks` if installed, else warn-skip)
- block files >1MB
- block bare TODOs
- block staging `.env` / pem-style secrets
- block staging test media binaries (`forbid-test-media.sh` — see [testing.md](testing.md))
- block staged **source** files **>1000 lines** (`forbid-long-source.sh` — split modules; see [code-style.md](code-style.md))

### commit-msg

Conventional Commits **format** only ([commits.md](commits.md)). English for commits/PRs is policy (`AGENTS.md`), not validated in this hook.

### pre-push

- `clippy --all-targets --all-features -D warnings` + `cargo nextest`/`cargo test`
  on the **affected set** (ci-affected.ts vs `origin/main`; NONE skips both,
  ALL runs the workspace) — a non-Rust push gates in ~2s instead of ~40s
- **wasm32 cross-cfg smoke**: the dev machine is Windows-only, so a cfg-gated
  break on non-Windows (e.g. a windows-only import in an example) passes every
  local check and fails CI's ubuntu job. A `cargo check --target
  wasm32-unknown-unknown --lib --bins --examples` on the affected ∩
  wasm32-clean crates compiles the same not-windows cfg paths (no C deps;
  benches excluded — criterion refuses wasm32)
- the test-media fixture cache (`local/.cache/test-media`) is **cleared** before
  tests: a cached fixture whose BLAKE3 still matches an outdated constant would
  pass locally and break CI — every push regenerates and re-verifies fixtures
- `cargo deny check advisories licenses bans sources`

## CI (GitHub Actions)

| Workflow | Trigger | Role |
|----------|---------|------|
| [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml) | `push` / `pull_request` → `main` | fmt · clippy · test · deny (merge gate) |
| [`.github/workflows/bench-daily.yml`](../../.github/workflows/bench-daily.yml) | **Daily cron** + `workflow_dispatch` on **`main` only** | Bench smoke (`ci-smoke`); skip if `main` unchanged since last success; artifacts (not PR noise) |
| [`.github/workflows/release.yml`](../../.github/workflows/release.yml) | push to `release` / `release/**` (+ `workflow_dispatch`, `dry_run` input) | Publish crates.io · npm · NuGet · PyPI · CPack, then tag `v<version>` + GitHub release (needs repo secrets — [repo-operations.md](repo-operations.md) § Publishing) |

Daily benches do **not** replace pre-push. Hosted runners are not `ref-*` GPU baselines — see [benchmarking.md](benchmarking.md) § Daily CI (`main`).

| Job | Runs on | Checks |
|-----|---------|--------|
| `rust` | `windows-latest`, `ubuntu-latest` | `fmt --check`, `clippy -D warnings`, `test`, source ≤1000 lines |
| `deny` | `ubuntu-latest` | `cargo deny` (advisories, licenses, bans, sources) |

Default CI stays light (no GPU, no system FFmpeg). Oracle / HW benches stay optional (see [testing.md](testing.md) · [benchmarking.md](benchmarking.md)).

### Caching (PR + main) — isolated namespaces

| Namespace | When | rust-cache `prefix-key` | sccache `SCCACHE_GHA_VERSION` |
|-----------|------|-------------------------|--------------------------------|
| **main** | `push` to `main` | `mediaway-ci-main-v1` | `mediaway-main-v1` |
| **pr** | `pull_request` | `mediaway-ci-pr-v1` | `mediaway-pr-v1` |

Both sides **save and restore**, but keys never overlap — PR cannot write into main’s cache line, and main’s restore-keys cannot pick up PR blobs.

- Shared **PR pool** (all PRs) is intentional: faster PR CI; still isolated from main.
- Cold-start: bump the `-v1` suffix on the namespace you want to purge (or both).
- Incremental **test selection** remains a **dev-loop** tool; PR CI runs the full workspace suite.

## Rules

- Do not pipe hook output through `tail`/`grep` — you lose failure diagnostics
- With `--no-verify`, put `[skip-hooks: <reason>]` in the commit body
- Cargo steps run in **sequential wrappers** (`cargo-precommit.sh` / `cargo-prepush.sh`) to avoid Windows cargo lock races
