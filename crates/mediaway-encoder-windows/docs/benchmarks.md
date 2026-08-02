# mediaway-encoder-windows — benchmarks

Methodology: [`docs/conventions/benchmarking.md`](../../../docs/conventions/benchmarking.md).
Harness: [Criterion](https://docs.rs/criterion) (`benches/wmf_h264_encode.rs`, `harness = false`).

```bash
cargo bench -p mediaway-encoder-windows
```

Criterion does its own warmup (3s) + many-sample measurement (5s, ~100 samples) per
[Fair measurement](../../../docs/conventions/benchmarking.md#fair-measurement); no
extra warmup logic is needed in the bench code. Each encoder session is opened once
per bench function (outside the timed closure) and never flushed mid-run, so every
timed iteration is one steady-state `push_frame` + drain of an already-open session.

## Workload

640×480, H.264, NV12, target bitrate 4,000,000 bps, 30 fps time base. Frame content
is a **static mid-gray buffer/texture** (all-128 luma/chroma) — same convention as
this crate's existing tests (`tests/av_fmp4_zc_smoke.rs`, `src/lib.rs` unit tests):
content doesn't matter to the push+drain cost being measured, only that MF accepts
and encodes it. **Caveat:** flat, unchanging content is unusually cheap for any H.264
encoder (near-all-skip macroblocks after the first frame) — these numbers are a raw
pipeline-throughput ceiling, not a natural-video encode benchmark. Audio not included.

## Path classes actually available on this crate today

- **`sw_wmf_h264_cpu`** — `VideoInputPreference::CpuUploadOk` for H.264 hardcodes
  Microsoft's inbox **software** H.264 encoder MFT (`CLSID_MSH264EncoderMFT`, see
  `src/wmf/video.rs::open_cpu`). This is genuinely CPU software encode, not "CPU
  upload feeding a hardware encoder" — this crate has no such path wired for H.264
  today, so `sw` is the honest label, not `copy`.
- **`zc_wmf_h264_dx11`** — hardware H.264 encoder MFT fed a DXGI NV12 surface
  directly (`MFCreateDXGISurfaceBuffer`, `src/wmf/dx11.rs`): no payload memcpy on the
  Mediaway side, Zero-Copy. **Not measurable on the `ad-hoc` machine below** — see
  Results.

## Results

| Bench | Class | Mode | mediaway | oracle_ref | machine_id | Commit | ffmpeg_ver | Notes |
|-------|-------|------|----------|------------|------------|--------|------------|-------|
| `sw_wmf_h264_cpu` | sw | steady | 129.65 µs/frame (Criterion 95% CI [129.16, 130.14] µs, 100 samples, ~45k iters) | 370 µs/frame (libx264 medium, rtime=0.111s/300f) | `ad-hoc` | — | N-121806-gb39989604 | `-c:v libx264 -preset medium -b:v 4000k -pix_fmt yuv420p`; ffmpeg input via `-f lavfi -i color=gray:size=640x480:rate=30`, 300 frames, `-benchmark`, output `-f null -`. Pixel format differs slightly (Mediaway NV12 vs libx264 native yuv420p) — documented, not hidden. |
| `zc_wmf_h264_dx11` | zc | — | **N/A on the available test hardware** | 1053 µs/frame, informational only (h264_nvenc, rtime=0.316s/300f) | `ad-hoc` | — | N-121806-gb39989604 | See "Why zc is N/A here" below. ffmpeg HW row is **not** a comparison (no Mediaway zc number exists on the available test hardware) — shown only to confirm NVENC itself works at the driver/CUDA level. `-c:v h264_nvenc -preset p4 -b:v 4000k` |

## Why `zc_wmf_h264_dx11` is N/A on the available test hardware

This crate's own pre-existing unit tests (`open_dx11_zero_copy_or_skip_without_hw`,
`open_hevc_av1_vp9_dx11_or_skip`) already skip gracefully in this exact way on the
same hardware — this is not a bug introduced by the bench. The bench enumerates **every**
present DXGI adapter (not just whichever `D3D_DRIVER_TYPE_HARDWARE` picks by
default) and tries to open a Zero-Copy H.264 encoder on each:

- **NVIDIA GeForce RTX 4090** — `WindowsVideoEncoder::open` fails with
  `EncodeError::Backend`. NVIDIA's current driver does not register a Media
  Foundation **encode** hardware transform for H.264 (NVENC is exposed through
  NVIDIA's own API/`ffmpeg`'s `h264_nvenc`, not as an `IMFTransform` HW MFT — the
  ffmpeg row above confirms NVENC itself works fine outside Media Foundation).
- **Intel UHD Graphics 770** — same `EncodeError::Backend` outcome; no working HW
  H.264 encoder MFT found either.

No numbers are fabricated for the missing cells; this is an honest capability gap on
this specific ad-hoc host, not a Mediaway defect. A machine with a GPU/driver that
does register a D3D11-aware HW encoder MFT (e.g. some Intel Quick Sync driver
revisions, or AMD AMF via MFT) should produce real `zc_wmf_h264_dx11` numbers with
this same bench unmodified.

## Machine (`ad-hoc`)

Full template per [`docs/benchmarks/machines.md`](../../../docs/benchmarks/machines.md#slot-template-copy-when-assigning-a-ref--machine).
This is the user's own dedicated Windows 11 desktop (not remote/shared/rented). It is
**not** claiming the `ref-windows-gpu-a` slot — promoting this box to a permanent
`ref-*` profile is a maintainer decision left open in
[`crates/mediaway-encoder-windows/docs/roadmap.md`](roadmap.md).

| Field | Value |
|-------|--------|
| Status | ad-hoc (not a `ref-*` slot) |
| OS | Windows 11 Pro, build 26100 |
| CPU | 13th Gen Intel(R) Core(TM) i9-13900K |
| RAM | 63.8 GB |
| GPU | NVIDIA GeForce RTX 4090; Intel UHD Graphics 770 (integrated) |
| GPU driver | NVIDIA 595.79 (`nvidia-smi`) |
| FFmpeg (PATH) | `ffmpeg version N-121806-gb39989604`, built with `--enable-nvenc --enable-nvdec --enable-cuvid --enable-amf` |
| Display / headless | Attached display, interactive session |
| Power plan | Balanced (not High performance — noted per methodology; not switched for this run) |
| Rust | `rustc 1.95.0 (59807616e 2026-04-14)`, host `x86_64-pc-windows-msvc` |
| Notes | Desktop also shows "Parsec Virtual Display Adapter" / "SudoMaker Virtual Display Adapter" in `Win32_VideoController` — unrelated remote-access software installed on the machine, not evidence of a rented/shared session; the machine is confirmed to be its owner's dedicated host, not rented or shared. Neither the NVIDIA nor the Intel adapter exposes a working Media Foundation HW encoder or decoder MFT for H.264 on the currently installed drivers (see Results). |
| Owner / location | User's own machine (this session) |
| Last verified | 2026-07-28 |
