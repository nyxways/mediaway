# Reference benchmark machines

Published Mediaway numbers must name a **machine profile id** so results are comparable. Full methodology: [`docs/conventions/benchmarking.md`](../conventions/benchmarking.md).

## Rules

1. **Every published row** includes `machine_id` (from this registry) **or** `ad-hoc` plus a full hardware block (see template below).
2. **Official baselines** (docs that claim “our baseline”) use a **`ref-*`** profile only — not an unnamed laptop.
3. **Ad-hoc / contributor machines** are fine in PRs; mark `machine_id: ad-hoc` and fill the template. Do not overwrite `ref-*` tables with ad-hoc numbers.
4. **Local detail** for *your* PC stays in gitignored [`local/machine/`](../../local/README.md); map it to a profile id when publishing.
6. Reference hosts used for encode/decode/mux baselines should keep a **documented system FFmpeg** on PATH (version pinned in the profile) so `oracle_ref` columns stay reproducible ([benchmarking.md](../conventions/benchmarking.md)).

## Profile registry

Fill concrete SKUs when a machine is dedicated. Until then, profiles are **slots** with required fields.

| `machine_id` | Tier | Role | Status |
|--------------|------|------|--------|
| `ref-windows-gpu-a` | Reference | Primary Windows HW encode / Zero-Copy baselines | **Slot** — assign CPU/GPU/driver when available |
| `ref-windows-cpu-a` | Reference | Windows software / sans-IO / no discrete GPU needed | **Slot** |
| `ref-web-chromium-a` | Reference | Browser / WebCodecs / WebGPU benches | **Slot** |
| `ref-linux-gpu-a` | Reference | Linux VA-API / Vulkan Video baselines | **Slot** (after Linux track) |
| `ci-smoke` | CI | Cheap non-HW or tiny pure benches in automation | Optional; must not claim HW Zero-Copy |
| `ad-hoc` | Informal | Any other machine — full template required in the PR/doc note | Always available |

### Slot template (copy when assigning a `ref-*` machine)

```markdown
### ref-windows-gpu-a

| Field | Value |
|-------|--------|
| Status | active |
| OS | Windows 11 … (build) |
| CPU | … |
| RAM | … |
| GPU | … |
| GPU driver | … (date/version) |
| FFmpeg (PATH) | `ffmpeg -version` summary — required for encode/decode/mux baseline hosts |
| Display / headless | … |
| Power plan | High performance (or note) |
| Rust | `rustc -Vv` summary / toolchain file pin |
| Notes | thermals, laptop vs desktop, docking, … |
| Owner / location | … |
| Last verified | YYYY-MM-DD |
```

Same fields for other `ref-*` ids. Keep this file in **English**.

## Result row shape

```markdown
| Bench | Class | Result | machine_id | Commit | Features | Notes |
|-------|-------|--------|------------|--------|----------|-------|
| `zc_wmf_h264_dx11` | zc | … ms/frame | `ref-windows-gpu-a` | `abc1234` | `…` | 1080p30 … |
```

If `machine_id` is `ad-hoc`, add a footnote or PR section with the slot template fields.

## Comparing numbers

| Allowed | Not allowed |
|---------|-------------|
| Same `machine_id` + same class (`zc` vs `zc`) across commits | `ref-*` vs `ad-hoc` as if equivalent |
| Show `zc` and `readback` side-by-side on the **same** machine | Cross-machine “we got faster” without stating both profiles |
| CI `ci-smoke` trends for pure/CPU microbenches | Using `ci-smoke` to claim discrete-GPU Zero-Copy wins |

## Related

- [`benchmarking.md`](../conventions/benchmarking.md)  
- [`local/README.md`](../../local/README.md) — per-developer machine notes  
- [maturity-bar.md](../spec/maturity-bar.md)
