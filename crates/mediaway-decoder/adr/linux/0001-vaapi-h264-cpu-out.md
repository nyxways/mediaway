# ADR-0001: VA-API via `cros-libva`, H.264 CPU-output decode

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder-linux`

## ⚠️ Zero real-hardware verification in this session

**Read this before relying on this crate.**

This crate was written and is compile-verified on Linux (WSL2 Ubuntu 24.04 via
`cargo check` / `cargo test` / `cargo clippy`, real `libva-dev` headers/bindgen output —
`mediaway-encoder-linux`'s ADR-0001 already confirmed `libva-dev` 2.20.0 (libva 1.20.0 API)
builds cleanly against `cros-libva` in this exact environment), but **`Display::open()` /
`vaInitialize` has been invoked against exactly zero real VA-API hardware**:

- This is a Windows dev box; it cannot run Linux VA-API natively.
- The WSL2 Ubuntu instance available in this session has **broken VA-API** (`vainfo`
  segfaults) and only software `llvmpipe` Vulkan — no real GPU is exposed to WSL.
- The user explicitly chose "ADR + cross-compile scaffolding only" over spending time on WSL
  GPU passthrough for this session.
- Every hardware-touching code path (`Display::open`, `Config`, `Surface`, `Context`,
  `Picture` begin/render/end/sync, `Image::create_from`/`vaGetImage`) is written to be
  **correct enough to run on a real Linux + VA-API machine**, grounded in the actual `va.h`
  struct layouts (fetched and read directly from `intel/libva`, not paraphrased from memory)
  and the actual `cros-libva` safe-wrapper source (including its own in-repo, HW-gated
  `libva_utils_mpeg2vldemo`/`enc_h264_demo` integration tests, used as a call-sequence
  reference) — but **none of it has been observed to succeed against real `vaInitialize`, a
  real driver, or a real decode session.**
- The honest-skip test (`open_vaapi_h264_cpu_or_skip`, mirroring the Windows
  `_or_skip` convention and `mediaway-encoder-linux`'s `vaapi_open_or_skip_without_hw`) is
  **expected to skip** in this session and in any CI environment without a real
  `/dev/dri/renderD*` VA-API device. A skip here is correct, not a bug.
- Treat every VA-API call path in this crate, and every H.264 bitstream-parsing decision
  below, as **unverified against a real encoder's output** until run on real hardware
  (Intel iHD / Mesa VA-API / AMD, per the project's platform-order Stage 3) with a real
  H.264 stream.

## Context

Linux hardware decode needs VA-API (`libva`) bindings. `mediaway-encoder-linux`'s ADR-0001
already reviewed and adopted [`cros-libva`](https://crates.io/crates/cros-libva)
(BSD-3-Clause, `deny.toml` allow-listed) for the encode side of this same crate pair; that
review (need / license / transitive license / maintenance / API stability / cost / unsafe
surface) is not repeated in full here — see that ADR. This ADR covers what is
**decode-specific**: VA-API's decode call sequence, H.264 bitstream parsing (which VA-API,
unlike Windows Media Foundation, requires the **application** to do — see below), and this
crate's scope.

### Why decode needs an H.264 bitstream parser (unlike the Windows CPU decode path)

`mediaway-decoder-windows`'s `CpuFramesOk` path (ADR-0001) hands Annex-B/AVCC bytes straight
to a Media Foundation software decoder MFT, which does **all** SPS/PPS/slice-header parsing,
entropy decoding, and reconstruction internally — Mediaway code never touches H.264 syntax.
VA-API is architected differently: the **driver** does entropy decoding + motion compensation
+ reconstruction, but the **application** must parse SPS/PPS/slice headers itself and build
`VAPictureParameterBufferH264` / `VASliceParameterBufferH264` / `VAIQMatrixBufferH264` from
those parsed fields. There is no way around writing an H.264 high-level-syntax parser for a
genuinely correct VA-API decode backend.

## Decision

> Depend on **`cros-libva` 0.0.13** (workspace-pinned) as a **`cfg(target_os = "linux")`**
> target dependency, exactly mirroring `mediaway-encoder-linux`'s gate. Reuse
> **[`mediaway_sw::h264`](../../mediaway-sw/docs/roadmap.md)'s `BitReader` and Annex-B/AVCC NAL
> splitting** (`split_annex_b`, `NalUnit::parse`) instead of re-implementing bit-level framing
> — see Alternatives for why that crate's own `Sps`/`Pps` types are *not* reused. Scope
> decode to a deliberately narrow, real, useful subset:

- **IDR pictures only** (`nal_unit_type == 5`), **single slice per picture**
  (`first_mb_in_slice == 0`), **I slices only**, **`pic_order_cnt_type == 0`**,
  **progressive** (`frame_mbs_only_flag == 1`), **baseline/main profile**
  (`profile_idc` 66 or 77). This eliminates DPB / reference-picture-list / MMCO / weighted-
  prediction / FMO handling entirely — an IDR I-slice picture references nothing, so
  `VAPictureParameterBufferH264::num_ref_frames == 0` and every `ReferenceFrames`/
  `RefPicList0`/`RefPicList1` entry is `VA_PICTURE_H264_INVALID`. Anything outside this
  subset returns `DecodeError::Unsupported`, never silently wrong output.
- New `vaapi::sps` / `vaapi::pps` / `vaapi::slice` modules parse exactly the raw syntax
  elements VA-API's parameter buffers need (`log2_max_frame_num_minus4`,
  `pic_order_cnt_type`, `pic_width_in_mbs_minus1`, …), built on `mediaway_sw::h264::BitReader`.
  PPS extension fields (`more_rbsp_data()`-gated `transform_8x8_mode_flag` / custom scaling
  lists) are read far enough to confirm absence; a stream that sets them returns
  `Unsupported` rather than being silently misdecoded.
- `VASliceParameterBufferH264::slice_data_bit_offset` is computed as `8 + bits consumed
  parsing the slice header` (the `8` is the NAL header byte — ITU-T H.264's VA-API buffer
  contract comment: "relative to and includes the NAL unit byte"); the `SliceData` buffer is
  the **original** (emulation-prevention-intact) NAL bytes, never the de-emulated copy used
  only for header parsing. Regression-tested with a hand-computed exact bit count
  (`slice_tests.rs`).
- Profile: `VAProfileH264ConstrainedBaseline` (from `profile_idc == 66`) or
  `[VAProfileH264Main, VAProfileH264ConstrainedBaseline]` (from `profile_idc == 77`, Main
  preferred, Constrained Baseline as fallback for drivers that don't enumerate Main
  separately), entrypoint `VAEntrypointVLD`. `VAConfigAttribRTFormat` = `VA_RT_FORMAT_YUV420`.
- IQ matrices: always the spec's default **flat 16** 4×4/8×8 lists — no custom scaling list
  parsing this session (`scaling_list()` syntax deferred; `Pps::parse` rejects a PPS that
  sets `pic_scaling_matrix_present_flag`).
- NV12 readback via `Picture::create_image` (`vaCreateImage` + `vaGetImage`, an explicit
  `VAImageFormat` matched to `VA_FOURCC_NV12` via `Display::query_image_formats()`) rather
  than `derive_image` (`vaDeriveImage`) — the same choice `cros-libva`'s own
  `libva_utils_mpeg2vldemo` reference test makes, and more portable ("not all Surfaces can be
  derived" per that crate's own doc comment on `derive_image`). This is a genuine
  driver→CPU copy — this crate's whole `CpuFramesOk` path is CPU-output, not Zero-Copy, so
  that copy is already accounted for, not a new hidden cost.
- Pipeline (`Config`/`Surface`/`Context`) creation is **lazy**: `open()` eagerly attempts the
  real `Display::open()` (the "attempt real hardware, fail honestly" requirement for this
  crate), but `vaCreateConfig`/`vaCreateSurfaces`/`vaCreateContext` need a profile + coded
  resolution that may not be known yet (`VideoDecoderConfig::extra_data` "may be empty until
  first keyframe") — they are created on the first in-band or `extra_data`-seeded SPS.
- `VideoOutputPreference::ZeroCopyGpu` returns `DecodeError::Unsupported` — DMA-BUF surface
  export is deferred (see roadmap).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Hand-written `bindgen` FFI in this crate | Same reasoning as `mediaway-encoder-linux` ADR-0001: reinvents `Display`/`Config`/`Context`/`Surface`/`Picture` typestate safety `cros-libva` already provides. |
| Reuse `mediaway_sw::h264::{Sps, Pps}` as-is | That crate's parser intentionally **discards** several raw syntax elements this crate's VA-API parameter buffers require (`log2_max_frame_num_minus4`, `pic_order_cnt_type`, `pic_order_cnt_lsb` bit-width, PPS `chroma_qp_index_offset`/`weighted_pred_flag`/…) — it only keeps what its own (future) software decode path needs. Re-implementing SPS/PPS/slice parsing here, on top of the *shared* `BitReader`/NAL-framing primitives (which are 100% reusable, format-level, and already tested), avoids duplicating the trickiest, least-reusable part (bit framing / emulation prevention) while keeping the VA-API-specific field set honest and local. |
| Hand-pack `seq_fields`/`pic_fields` bitfield unions manually (`.value = packed_u32`) | Unnecessary — `cros-libva`'s `buffer::h264` module already exposes safe, named constructors (`H264SeqFields::new`, `H264PicFields::new`) that build the bindgen bitfield unions correctly; using them avoids re-deriving the exact bit-packing order from `va.h` by hand. |
| Support general (non-IDR, multi-reference) H.264 decode this session | Would require a DPB, reference picture list construction (8.2.4), MMCO/sliding-window marking, and POC state across pictures — multiple times this session's scope for a stage explicitly asking for "minimal real implementation," per the task's own instruction to keep scope to baseline/main, CPU-output only. Deferred to Stage 2 (roadmap). |
| `vaDeriveImage` instead of `vaCreateImage`+`vaGetImage` | Zero-copy view *of the surface*, but "not all Surfaces can be derived" (cros-libva's own doc comment) and requires an extra runtime fourcc check; `vaGetImage` with an explicitly-queried `VA_FOURCC_NV12` format is the more portable, already-demonstrated-working (in `cros-libva`'s own test) choice for a CPU-output path that is already a copy either way. |

## Consequences

### Positive

- Small, real unsafe surface: **zero** `unsafe` blocks written in this crate (all FFI unsafety
  lives in `cros-libva`) — this crate uses `#![forbid(unsafe_code)]` at the crate root.
- Typestate `Picture` flow makes an invalid VA-API call order a compile error, not a runtime
  bug class.
- IDR-only scope means no DPB at all — eliminates an entire class of reference-management
  bugs for this stage, while still being a genuinely useful capability (keyframe/I-frame
  decode).
- Reusing `mediaway_sw::h264`'s bit reader/NAL framing avoids re-testing/re-debugging the
  fiddliest, most bug-prone part of any H.264 parser from scratch.
- Structural parity with `mediaway-encoder-linux` (surface pool + fresh-replacement-on-error
  pattern, lazy pipeline creation, `&self`-based decode-then-return-surface shape) eases
  future cross-platform dispatch work and review.

### Negative / Trade-offs

- `cros-libva` 0.0.x: no semver stability guarantee yet — a future minor bump could break this
  crate and require re-review (same trade-off `mediaway-encoder-linux` already accepted).
- Build-time hard dependency on system `libva-dev` (+ `libva-drm`) on any Linux build of this
  crate — acceptable per this crate's `cfg(target_os = "linux")` gate (never required for
  Windows/Web/other builds).
- **IDR-only decode cannot play back a real-world GOP structure** (P/B frames) — only useful
  for all-intra streams or keyframe extraction until Stage 2 lands reference-picture support.
- Annex-B assumed for both `push_packet` and `extra_data`; AVCC-framed (length-prefixed)
  demuxer output is not handled this session — same open item
  `mediaway-decoder-windows`'s ADR-0001 already tracks for its own crate.
- **Zero hardware verification** (see caveat above) — real-world correctness, including the
  exact `slice_data_bit_offset` arithmetic and profile negotiation, is unproven until run on
  actual VA-API hardware with a real encoder's bitstream.

## References

- [`docs/conventions/deps-policy.md`](../../../docs/conventions/deps-policy.md)
- [`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md)
- `mediaway-encoder-linux` ADR-0001 (`cros-libva` dependency review, shared for this crate)
- `mediaway-decoder-windows` ADR-0001 (CPU-output decode precedent, Annex-B/AVCC open item)
- [`cros-libva` on crates.io](https://crates.io/crates/cros-libva) ·
  [GitHub](https://github.com/chromeos/cros-libva) (BSD-3-Clause)
- [`intel/libva` `va.h`](https://github.com/intel/libva/blob/master/va/va.h) — H.264
  structure layouts read directly from source for this ADR
- `docs/roadmap.md` § Linux (VA-API and/or Vulkan Video)
