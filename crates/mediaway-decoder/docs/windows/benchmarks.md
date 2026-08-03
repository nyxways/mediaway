# mediaway-decoder-windows — benchmarks

Methodology: [`docs/conventions/benchmarking.md`](../../../docs/conventions/benchmarking.md).
Harness: [Criterion](https://docs.rs/criterion) (`benches/wmf_h264_decode.rs`, `harness = false`).

```bash
cargo bench -p mediaway-decoder-windows
```

Real compressed H.264 bytes are produced once per bench (untimed setup) by encoding
30 synthetic NV12 frames through `mediaway-encoder-windows`'s CPU (software) encoder
— same approach as `tests/cpu_roundtrip.rs` — rather than any committed media file.
Each Criterion iteration opens a **fresh** decoder session via `iter_batched` (the
session-open call is untimed setup, per Criterion's own setup/measure split) and
times only the steady push+drain+flush of that one 30-frame sequence, since a
decoder session cannot be reused for a second sequence once flushed
(`DecodeError::Closed` after `flush`). Reported per-batch time divided by 30 gives
ms/frame.

## Workload

640×480, H.264, NV12 output, 30 fps time base, 30-frame batch per iteration. Source
frames are a **static mid-gray buffer** (all-128 luma/chroma) — content doesn't
matter to the decode throughput measured here, only that MF accepts and decodes the
resulting bitstream. **Caveat:** flat, unchanging content is unusually cheap to
decode (near-all-skip macroblocks) — these numbers are a raw pipeline-throughput
ceiling, not a natural-video decode benchmark. Audio not included.

## Path classes actually available on this crate today

- **`sw_wmf_h264_cpu`** — the synchronous software H.264 decoder MFT
  (`src/wmf/cpu.rs::open_sw_decoder`); no GPU device anywhere in the chain, so this
  is honest CPU decode, not a GPU→CPU readback (matches that module's own doc
  comment).
- **`zc_wmf_h264_dx11`** — hardware H.264 decoder MFT with DXGI output surfaces
  (`src/wmf/dx11.rs`): decoded frames stay GPU-resident, Zero-Copy. **Not
  measurable on the `ad-hoc` machine below** — see Results.

## Results

| Bench | Class | Mode | mediaway | oracle_ref | machine_id | Commit | ffmpeg_ver | Notes |
|-------|-------|------|----------|------------|------------|--------|------------|-------|
| `sw_wmf_h264_cpu` | sw | steady | 497.5 µs/frame (14.925 ms / 30-frame batch, Criterion 95% CI [480.2, 516.1] µs/frame, 100 samples) | 63.3 µs/frame (ffmpeg sw H.264 decoder, rtime=0.019s/300f) | `ad-hoc` | — | N-121806-gb39989604 | Input: 300-frame H.264 file produced by `ffmpeg -f lavfi -i color=gray:size=640x480:rate=30 -c:v libx264 -preset medium -b:v 4000k`; decode timed with `ffmpeg -i <file> -f null -benchmark -`. ffmpeg's native libavcodec H.264 decoder is meaningfully faster than the Media Foundation inbox software decoder MFT here — reported as-is. |
| `zc_wmf_h264_dx11` | zc | — | **N/A on the available test hardware** | 347 µs/frame, informational only (h264_cuvid/NVDEC, rtime=0.104s/300f) | `ad-hoc` | — | N-121806-gb39989604 | See "Why zc is N/A here" below. ffmpeg HW row is **not** a comparison (no Mediaway zc number exists here). `-hwaccel cuda -hwaccel_output_format cuda -c:v h264_cuvid`. Note NVDEC's own number is *slower* than ffmpeg's sw decode for this same tiny/trivial 300-frame flat-content job — HW session init/CUDA context overhead isn't amortized at this job size; not a claim that NVDEC is generally slower than software decode. |

## Why `zc_wmf_h264_dx11` is N/A on the available test hardware

This crate's own pre-existing unit test (`open_dx11_zero_copy_or_skip`) already skips
gracefully in this exact way — this is not a bug introduced by the
bench. The bench enumerates **every** present DXGI adapter (not just whichever
`D3D_DRIVER_TYPE_HARDWARE` picks by default) and tries to open a Zero-Copy H.264
decoder on each:

- **NVIDIA GeForce RTX 4090** — `WindowsVideoDecoder::open` fails with
  `DecodeError::Unsupported` (no D3D11-aware HW decoder MFT found/registered for
  Media Foundation on this driver, even though NVDEC itself works fine outside MF —
  see the ffmpeg `h264_cuvid` row above).
- **Intel UHD Graphics 770** — same `DecodeError::Unsupported` outcome.

No numbers are fabricated for the missing cell; this is an honest capability gap on
this specific ad-hoc host, not a Mediaway defect. A machine whose driver stack
registers a D3D11-aware HW decoder MFT should produce real `zc_wmf_h264_dx11`
numbers with this same bench unmodified.

## Machine (`ad-hoc`)

Same host as [`mediaway-encoder-windows/docs/benchmarks.md`](../../mediaway-encoder-windows/docs/benchmarks.md#machine-ad-hoc)
— full template per [`docs/benchmarks/machines.md`](../../../docs/benchmarks/machines.md#slot-template-copy-when-assigning-a-ref--machine),
reproduced here for discoverability. The user's own dedicated Windows 11 desktop
(not remote/shared/rented); not claiming the `ref-windows-gpu-a` slot — that
promotion is a maintainer decision left open in
[`crates/mediaway-decoder-windows/docs/roadmap.md`](roadmap.md).

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
| Notes | Desktop also shows "Parsec Virtual Display Adapter" / "SudoMaker Virtual Display Adapter" in `Win32_VideoController` — unrelated remote-access software installed on the machine, not evidence of a rented/shared session; confirmed by the user to be their own dedicated box. Neither GPU exposes a working Media Foundation HW encoder or decoder MFT for H.264 on the currently installed drivers (see Results). |
| Owner / location | User's own machine (this session) |
| Last verified | 2026-07-28 |
