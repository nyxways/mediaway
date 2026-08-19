# Linux decode (VA-API)

- Module: `mediaway-decoder::linux`
- Bindings: [`cros-libva`](https://crates.io/crates/cros-libva) (BSD-3-Clause, `cfg(target_os =
  "linux")` dependency only — never pulled into non-Linux builds)
- Codec: H.264 baseline/main (`vaapi/codec.rs` → `VAProfileH264ConstrainedBaseline` /
  `VAProfileH264Main`, `VAEntrypointVLD`), AV1 Main profile `KEY_FRAME`-only
  (`VAProfileAV1Profile0`, `VAEntrypointVLD`), and VP9 Profile 0 `KEY_FRAME` + general
  `INTER_FRAME` (`VAProfileVP9Profile0`, `VAEntrypointVLD`) — dispatched via
  `vaapi::VaapiVideoDecoder`, an enum over the three per-codec decoders (no
  `Box<dyn VideoDecoder>`)
- Scope: I and single-forward-reference P slices (real GOP/IPPP... decode), single slice per
  picture, `pic_order_cnt_type == 0`, progressive. Sliding-window DPB (`vaapi/dpb.rs`, ported
  from `vulkan/dpb.rs`), `RefPicList0[0]`-only reference (no reordering, no long-term refs, no
  weighted prediction, no CABAC P-slices, no B-slices, no multi-reference — all rejected
  honestly). Own H.264 SPS/PPS/slice-header parser (`vaapi/sps.rs`/`pps.rs`/`slice.rs`) reuses
  [`mediaway_sw::h264`](../../../crates/mediaway-sw/docs/roadmap.md)'s `BitReader` + Annex-B
  NAL splitting — VA-API (unlike Windows Media Foundation) requires the *application* to
  parse SPS/PPS/slice headers itself.
- CPU: NV12 readback via `Picture::create_image` (`vaCreateImage`+`vaGetImage`) — pitches/
  offsets stripped by `vaapi/nv12.rs` into a tightly packed buffer (same layout as
  `mediaway-decoder::windows`'s `wmf/cpu.rs`)
- Zero-Copy: **not implemented** — `VideoOutputPreference::ZeroCopyGpu` returns `Unsupported`
  (deferred: DMA-BUF surface export, `vaExportSurfaceHandle`)
- Session shape: `Display`/`Config`/`Context`/`Surface` (safe `cros-libva` wrappers) +
  `Picture<S, T>` **typestate** enforcing `vaBeginPicture → vaRenderPicture → vaEndPicture →
  vaSyncSurface` ordering at compile time; pipeline creation is **lazy** (first SPS seen),
  since `open()` cannot know profile/coded resolution before that
- ADR: [0001](../../../../crates/mediaway-decoder/adr/linux/0001-vaapi-h264-cpu-out.md) —
  binding choice, decode scope, **zero real-hardware verification** caveat
- ADR: [0002](../../../../crates/mediaway-decoder/adr/linux/0002-vaapi-h264-p-slice-dpb.md) —
  **Implemented, WSL2 compile+test-verified**: extends decode to
  single-forward-reference P-slices (+ free non-IDR I-slices) by porting the
  sliding-window DPB / POC arithmetic from `vulkan/dpb.rs` /
  `vulkan/h264_slice.rs` / `vulkan/h264_params.rs` into a new, sans-io
  `vaapi/dpb.rs`; no B-slices, no reference-list reordering, no weighted
  prediction, no CABAC P-slices, no multi-reference this round. Surface pool
  is DPB-slot-indexed (round-robin removed), sized `sps.max_num_ref_frames +
  1`. Found two real latent gaps in the Vulkan porting source itself
  (`pred_weight_table()`, `cabac_init_idc` unhandled) — rejected honestly
  here rather than inherited silently. Still zero real VA-API hardware
  verification.
- ADR: [0003](../../../../crates/mediaway-decoder/adr/linux/0003-vaapi-av1-key-frame-decode.md) —
  **Implemented, WSL2 compile+test-verified**: AV1 `KEY_FRAME`-only decode (`vaapi/av1.rs` +
  `vaapi/av1/{obu,bits,sequence_header,frame_header,tile_info}.rs`), single tile, Main profile,
  every optional coding tool (segmentation/film grain/CDEF/loop restoration/superres/warped
  motion) rejected as `Unsupported` if signaled. No AV1 decode existed anywhere in this
  workspace to port from — the bitstream parser (`sequence_header_obu()`/`uncompressed_header()`)
  is spec-derived, cross-checked (not copied) against
  `mediaway-encoder::windows::d3d12_video_encode::bitstream_av1`'s OBU **writer** (inverse
  direction) and `mediaway_sw::h264::BitReader` (raw-bit primitive reuse; new local
  `uvlc()`/`su(n)`/`ns(n)` decoders for AV1's own variable-length codes). No `cros-libva` gap on
  the decode side (unlike its encoder sibling) — decode never needs a packed-header buffer;
  `VAProfileAV1Profile0`/`VAEntrypointVLD`/`VAAV1TransformationType` all resolved correctly
  against real WSL2 bindgen output on the first compile pass. Accepts both legal AV1 framings
  (`OBU_FRAME`, and the split `OBU_FRAME_HEADER` + `OBU_TILE_GROUP` pair). `INTER_FRAME`/GOP
  decode explicitly deferred: harder here than H.264's same-pass DPB extension (no porting
  precedent, AV1's CDF-forward-adaptation + 9-way reference-slot model). **Zero real VA-API
  hardware verification**, same caveat as ADR-0001/0002.
- ADR: [0004](../../../../crates/mediaway-decoder/adr/linux/0004-vaapi-vp9-key-frame-and-inter-decode.md) —
  **Implemented, WSL2 compile+clippy+test-verified**: VP9 `KEY_FRAME` + **general**
  `INTER_FRAME` decode (no artificial reference-count restriction, unlike H.264's
  `RefPicList0[0]`-only scope — `reference_frames[8]` is always fully populated regardless of
  active-reference count, confirmed against `FFmpeg`'s `vaapi_vp9.c`). Broader scope than the
  AV1 sibling's `KEY_FRAME`-only cut: VP9 entropy adaptation is driver-internal (four
  passthrough scalars, no app-side CDF math), and its reference model is a flat, spec-fixed
  8-slot array (2 fields/slot: width/height, `vaapi/vp9/ref_table.rs`) vs. AV1's
  12-field-per-slot state. New `vaapi/vp9/{bits,color_config,frame_size,header,loop_filter,
  quantization,ref_table,segmentation,tile_info}.rs` tree, `#![forbid(unsafe_code)]`, 100+
  hand-constructed bitstream fixture tests. **Real correction found this session**: an earlier
  open question had assumed VP9's `su(n)` matches AV1's sign-embedded-in-top-bit shape; the
  primary spec text (`pdftotext`-extracted this pass) confirms VP9 instead uses `s(n)` — a plain
  `f(n)` magnitude read then a **separate** sign bit (`n + 1` bits, not `n`). `RefTable::
  free_pool_index` uses a `POOL_SIZE = VP9_REF_SLOTS + 1 = 9` pigeonhole guarantee so multiple
  logical slots can alias one physical picture (this workspace's own VP9 encoder sibling's
  ping-pong output legitimately refreshes 7 of 8 logical slots per `INTER_FRAME`) without
  needing 8 separate physical buffers. **Zero real-hardware verification**, same caveat as
  every ADR in this folder.

## ⚠️ Hardware verification status

Written and compile-verified on Linux (WSL2 Ubuntu 24.04, real `libva-dev` headers/bindgen
output) but **never run against a real VA-API display/driver**. The dev box is Windows (no
native VA-API); the available WSL2 instance has broken VA-API (`vainfo` segfaults) and only
software `llvmpipe` Vulkan. `Display::open()` honestly returns `DecodeError::Unsupported` in
that environment — this is the expected, documented outcome, not a bug. Treat every VA-API
call path, and every H.264 bitstream-parsing decision (especially the
`slice_data_bit_offset` arithmetic), as unverified against a real encoder's bitstream until
run on real Linux + VA-API hardware (Intel iHD / Mesa / AMD).

## Structural differences vs. Windows (WMF) CPU decode

| Windows | Linux | Note |
|---------|-------|------|
| Software H.264 decoder MFT parses the whole bitstream internally | Driver does entropy decode + reconstruction; **this crate** parses SPS/PPS/slice headers | VA-API's app/driver split, not MF's black-box MFT |
| `IMFTransform` (one object) | `Config` + `Context` (capability vs. bound session) | VA-API separates driver capability from session |
| Async event pump for HW MFTs | None needed | `vaSyncSurface` blocks; fully synchronous per picture |
| DX11 Zero-Copy decode out | Not implemented this stage | See ADR-0001 § Scope |
