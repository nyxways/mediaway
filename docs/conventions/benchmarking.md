# Benchmarking

Performance claims need **numbers**, not vibes. Benchmarks are part of the [maturity bar](../spec/maturity-bar.md) and must stay **honest** about Zero-Copy vs copy/readback paths ([caveats-and-clarity.md](../spec/caveats-and-clarity.md)).

**Reference machines:** [`docs/benchmarks/machines.md`](../benchmarks/machines.md) — every published result names a `machine_id`.

## Principles

1. **Separate paths.** Never mix Zero-Copy (`zc` — GPU **or** shared CPU) with CPU readback or cross-API copy benches in one unlabeled number.
2. **Name the cost.** Bench IDs and reports use clear labels: `zc_…`, `copy_…`, `readback_…`, `sw_…`.
3. **Name the machine.** Use a registry `machine_id` (`ref-*`, `ci-smoke`, or `ad-hoc` + full hardware block). See [machines.md](../benchmarks/machines.md).
4. **Reproducible inputs.** Prefer `mediaway-test-media` generators + BLAKE3 (same rules as [testing.md](testing.md)); no mystery blobs in git.
5. **Default CI stays light.** Full HW benches are optional/nightly or manual; `cargo test` must not require a discrete GPU or external tools.
6. **No silent slow path** inside a “fast” bench. If the setup copies GL→DX or maps to CPU, the bench name and rustdoc must say so.
7. **English** reports and harness comments.
8. **Official baselines** only from `ref-*` profiles — never promote an unnamed laptop into “the” baseline table.
9. **Reference tool column.** For benches that have a meaningful industry comparison (encode/decode/mux wall time, fps, etc.), publish **Mediaway and system `ffmpeg` on the same `machine_id`** side by side. FFmpeg is a **dev/oracle** binary only ([ADR-0002](../adr/0002-system-oracle.md)) — not a Cargo dep and not the product path.
10. **Fair measurement.** Prefer steady-state / amortized work over one-shot process cost. Account for **FFmpeg CLI process overhead** and other non-comparable setup. See [Fair measurement](#fair-measurement) below.

## Layout (when benches exist)

| Location | Role |
|----------|------|
| `crates/<name>/benches/` | Criterion (or chosen harness) benches for that crate |
| Crate `docs/benchmarks.md` | How to run + latest baseline table (**include `machine_id`**) |
| [`docs/benchmarks/`](../benchmarks/) | Shared machine registry + cross-crate notes |

Register with Cargo:

```toml
[[bench]]
name = "mux_throughput"
harness = false   # when using Criterion
```

Prefer **Criterion** on Rust crates unless an ADR picks another harness for a platform (e.g. browser).

## What to measure (by layer)

| Layer | Useful metrics |
|-------|----------------|
| Sans-IO mux/demux | Throughput (MB/s or packets/s), allocation rate if relevant |
| Encode / decode | End-to-end latency, frames/s, queue depth; **GPU residency** vs readback |
| Device capture | Time-to-first-frame, steady-state fps, drop count |
| Interop adapters | Cost of `copy_…` / import-export vs native handle path |

Always state **resolution, codec, bitrate/preset, pixel format**, and whether audio is included.

## Path classes (required labels)

| Class | Meaning | Example id |
|-------|---------|------------|
| `zc` | Zero-Copy — GPU handle **or** shared CPU buffer (no payload memcpy) | `zc_wmf_h264_dx11`, `zc_wasapi_pcm_share` |
| `copy` | Explicit CPU↔CPU or GPU↔GPU / API translation copy | `copy_bgra_nv12`, `copy_gl_to_dx11` |
| `readback` | GPU→CPU (and maybe back) | `readback_encode_cpu` |
| `sw` | Software codec / CPU encode | `sw_openh264` |
| `pure` | Pure CPU sans-IO (no device) | `pure_mp4_mux` |

Comparing `zc_*` to `readback_*` is fine **if both are shown** and the narrative does not pretend they are the same class.

## Machine profiles (summary)

| Kind | `machine_id` | Use |
|------|--------------|-----|
| Reference | `ref-windows-gpu-a`, `ref-windows-cpu-a`, `ref-web-chromium-a`, `ref-linux-gpu-a`, … | Official baseline tables |
| CI | `ci-smoke` | Tiny/pure automation only |
| Informal | `ad-hoc` | PRs / personal boxes — full HW template required |

Your PC’s private inventory: `local/machine/` (gitignored). When you publish, either map to a `ref-*` or use `ad-hoc` + template.

## Reference tool (system FFmpeg) beside Mediaway

When a scenario is comparable (same input, resolution, codec family, and a clear wall-clock or fps metric):

| Requirement | Detail |
|-------------|--------|
| Same machine | Identical `machine_id` for Mediaway and FFmpeg rows |
| Same workload | Same generator fixture / BLAKE3 input; document flags |
| Record versions | `ffmpeg -version` (first line / n-build) in Notes or a Versions row |
| Label clearly | Columns `mediaway` vs `oracle_ref`; include **Mode** (`steady` / `amortized` / `cold` / `overhead`) |
| Fair timing | Follow [Fair measurement](#fair-measurement) — no warm-library vs cold-CLI headline |
| HW honesty | Same **path class** for headlines (`zc`↔`zc`); see Fair measurement · Like-for-like |
| Missing FFmpeg | Official `ref-*` baseline updates that omit FFmpeg need an explicit reason (`ffmpeg not installed`, `N/A: pure sans-IO microbench`, …) |

### Table shape (preferred)

```markdown
| Bench | Class | Mode | mediaway | oracle_ref | machine_id | Commit | ffmpeg_ver | Notes |
|-------|-------|------|----------|------------|------------|--------|------------|-------|
| `zc_wmf_h264_1080p` | zc | steady | … ms/frame | … ms/frame | `ref-windows-gpu-a` | `abc1234` | n7.1 … | warmup 1; 600 frames; ffmpeg args: … |
```

Pure sans-IO microbenches (`pure_*`) may omit `oracle_ref` when there is no sensible FFmpeg counterpart — mark `N/A` once in Notes.

Scripted compares (optional later) may live under crate benches or `tools/`; they must invoke **PATH** `ffmpeg`, never link libav.

## Fair measurement

Comparisons must measure **the same kind of work**. Misleading wins (e.g. warm in-process Mediaway vs cold `ffmpeg` process startup) are **Blocking** in review.

### CLI / process overhead (FFmpeg and similar)

Spawning `ffmpeg` pays process create, dynamic linker, arg parse, demuxer/encoder init, and teardown. An in-process Mediaway API call does not. Rules:

| Rule | Detail |
|------|--------|
| Do not compare one-shot CLI wall time to one-shot library API as if equal | Always disclose both measurement modes |
| Prefer **steady-state** | Long enough input (many frames / large mux) so startup is a small fraction of total time; report **per-frame**, **per-second of media**, or **throughput** |
| Or **amortize explicitly** | Warmup: one discarded `ffmpeg` run (or Mediaway session open); then time N iterations / long job; report mean of timed portion only |
| Optional: isolate overhead | Publish a separate row `ffmpeg_cli_overhead` (empty/minimal job or `-f lavfi -i nullsrc=… -frames:v 1`) so readers see spawn+init cost — **not** mixed into the encode fps headline |
| Prefer long jobs for head-to-head | e.g. ≥ a few hundred frames at target resolution when claiming encode parity |
| Document the clock | Wall time of child process vs Criterion in-process; CPU time if used |

### Like-for-like path class (Zero-Copy)

**Do not** present Mediaway `zc` (Zero-Copy — GPU-resident **or** shared CPU) as beating FFmpeg when the FFmpeg side is software encode, CPU readback, unmatched pipeline, or a different path class.

| Mediaway class | Fair `oracle_ref` | Unfair as primary headline |
|----------------|-------------------|----------------------------|
| `zc` | FFmpeg **HW** path that keeps frames on GPU as far as that build allows (document `-hwaccel`, `-c:v h264_nvenc` / `h264_amf` / `h264_qsv` / D3D11VA, etc.) | FFmpeg **libx264** / pure SW while calling it the same race |
| `sw` | FFmpeg SW encode (e.g. libx264) with stated preset/CRF | Claiming SW-vs-HW as identical |
| `readback` / `copy` | Same class of cost on FFmpeg, or show both classes explicitly | Hiding that Mediaway paid a copy and FFmpeg did not (or vice versa) |
| `pure` | Often N/A for FFmpeg | — |

Rules:

1. **Headline comparisons** must share the same path **class** (`zc` vs `zc`, `sw` vs `sw`). Put the class on **both** Mediaway and `oracle_ref` columns/notes.
2. If FFmpeg cannot express an equivalent Zero-Copy path on that OS/GPU, either:
   - run the closest HW path and label residual copies, or
   - mark `oracle_ref` as `N/A — no equiv ZC` and publish Mediaway `zc` alone (plus optional separate `sw` head-to-head).
3. **Cross-class tables are allowed** (e.g. “our ZC vs their SW”) only when **both** numbers appear with distinct class labels — never as a single “fps winner” without that context.
4. Prefer documenting FFmpeg HW args that match the intent of Mediaway’s backend (Windows: prefer D3D11/NVENC/QSV/AMF aligned with the Mediaway path under test).

### Apples-to-apples checklist

Before publishing Mediaway vs `oracle_ref`:

1. **Same `machine_id`**, power plan, and (for GPU) similar thermal state (note if laptop on battery).
2. **Same input bytes** (BLAKE3 fixture) and stated output constraints (resolution, fps, bitrate/CRF/preset, pix fmt, audio on/off).
3. **Same path class** for headline claims — see [Like-for-like path class](#like-for-like-path-class-zero-copy--hw) (`zc`↔`zc`, `sw`↔`sw`).
4. **Exclude unrelated I/O** when possible — tmpfs/RAM disk or warm cache; same filesystem for both; do not time “download fixture” inside the timed region.
5. **Pin threads / affinity** only if both sides use the same policy; document it.
6. **Drop first iteration** (warmup) for both Mediaway and FFmpeg unless the metric is explicitly “cold start”.
7. **Report N and variance** (Criterion summary or mean±stddev), not a single lucky run.
8. **State FFmpeg args in full** in Notes or a linked command block (reproducible), including HW accel flags when class is `zc`.

### Cold start (when you intentionally measure it)

Cold-start latency is a valid **separate** metric (`cold_*` suffix), for both Mediaway session creation and `ffmpeg` first process. Never use cold CLI vs warm library as the primary “we are faster” headline.

### Measurement modes (label in Notes)

| Mode | Meaning |
|------|---------|
| `steady` | Warmup done; timed region is long job / many iterations (default for head-to-head) |
| `amortized` | Total time / N after warmup |
| `cold` | Includes process or session start (explicit only) |
| `overhead` | CLI/spawn probe only |

## Publishing results

```markdown
| Bench | Class | Mode | mediaway | oracle_ref | machine_id | Commit | ffmpeg_ver | Notes |
|-------|-------|------|----------|------------|------------|--------|------------|-------|
| `zc_wmf_h264_1080p` | zc | steady | … ms/frame | … ms/frame | `ref-windows-gpu-a` | `abc1234` | n7.1 … | warmup 1; 600 frames |
```

- Prefer ranges or Criterion summaries over single magical floats.
- Note thermal/boost caveats for short runs when relevant.
- Regressions that change a claimed baseline need a PR note (why slower/faster).
- Changing drivers/SKU on a reference host → update [`machines.md`](../benchmarks/machines.md) (bump id or date the change).
- Publish baselines in crate `docs/benchmarks.md` (markdown tables). Raw dumps stay under `local/benches/` (gitignored) — **do not** commit time-series JSON/HISTORY trees in-repo for v0/v1.

### Daily CI (`main`)

Workflow: [`.github/workflows/bench-daily.yml`](../../.github/workflows/bench-daily.yml).

| Rule | Detail |
|------|--------|
| Branch | **`main` only** (schedule on default branch; `workflow_dispatch` refused on other refs) |
| Cadence | Once per day (cron) + manual dispatch |
| Skip | No new commits on `main` since the last **successful** `bench-daily` run |
| Machine | `ci-smoke` on GitHub-hosted Ubuntu — **not** HW/`zc` baselines |
| Output | CI **artifacts** only (`local/benches/ci-smoke/`, `target/criterion/` when present) |
| PR CI | Does **not** run this workflow (avoids noisy / unfair shared-runner benches on every PR) |

### Incremental (dev)

Selective benches for the **local edit loop** (`cargo-impact` / git blast radius; Bun wrapper).

```bash
cd tools/scripts
bun run incremental-bench.ts                 # since HEAD + uncommitted
bun run incremental-bench.ts --since main
bun run incremental-bench.ts --no-deps       # only directly changed packages
bun run incremental-bench.ts -- --bench foo  # passthrough to cargo bench
```

| Rule | Detail |
|------|--------|
| Selection | Changed workspace packages (via `cargo impact --context` or `git diff`) + **reverse-deps** that declare `[[bench]]` |
| `--no-deps` | Skip reverse-dep expansion |
| Gate | **Never** pre-push / PR CI — full or daily `bench-daily` stays separate |
| Empty | No impacted `[[bench]]` → exit 0 |

Incremental tests: [testing.md](testing.md) § Incremental (dev).

## Commands (conventional)

```bash
# Example once benches exist
cargo bench -p iso-bmff
cargo bench -p mediaway-container
cargo bench -p mediaway-encoder-windows --bench zc_wmf_h264 -- --quick   # if supported
```

Document exact commands in the crate’s benchmark doc. Optional env (examples):

| Env | Purpose |
|-----|---------|
| `MEDIAWAY_BENCH_FILTER` | Subset of benches (if harness supports) |
| `MEDIAWAY_BENCH_OUT` | Write Criterion/HTML or JSON under a gitignored path |
| `MEDIAWAY_BENCH_MACHINE` | Optional hint of `machine_id` for report headers |

Do **not** commit Criterion `target/` criterion history blobs. Summarize published numbers in crate `docs/benchmarks.md`. Raw outputs may live under `local/benches/`.

## PR / review rules

- New perf-sensitive code: add or update a bench **or** explain in the PR why measurement is deferred (issue link).
- Changing a hot path: run relevant benches and paste summary in the PR when hardware is available; otherwise mark “benches not run (no HW)” honestly.
- Published or baseline-updating numbers must include **`machine_id`** (+ ad-hoc template if not `ref-*`).
- Comparable encode/decode/mux baselines must include **`oracle_ref`** on the same machine (or an explicit N/A reason), measured under **fair** rules (steady/amortized; CLI overhead; **same path class** for headlines — `zc`≠`sw`).
- Reviewer **Blocking**: perf win without methodology; copy/readback sold as Zero-Copy; official baseline from unnamed/`ad-hoc` without disclosure; comparable baseline without FFmpeg side-by-side / without N/A; **unfair** compares (warm in-process vs cold CLI; **Mediaway `zc` vs FFmpeg SW as a like-for-like win**).

## Relationship to tests

| Tests | Benchmarks |
|-------|------------|
| Correctness, determinism | Throughput / latency |
| Must pass without GPU/oracle by default | May need GPU; optional in CI |
| BLAKE3 fixtures | Same generators OK; measure time, not only hash |

## Related

- [machines.md](../benchmarks/machines.md)  
- [maturity-bar.md](../spec/maturity-bar.md)  
- [caveats-and-clarity.md](../spec/caveats-and-clarity.md)  
- [gpu-interop.md](../spec/gpu-interop.md)  
- [testing.md](testing.md)  
- [local-workspace.md](local-workspace.md)
