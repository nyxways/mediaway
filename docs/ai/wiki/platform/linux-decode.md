# Linux decode (VA-API)

- Module: `mediaway-decoder::linux`
- Bindings: [`cros-libva`](https://crates.io/crates/cros-libva) (BSD-3-Clause, `cfg(target_os =
  "linux")` dependency only — never pulled into non-Linux builds)
- Codec: H.264 baseline/main, HEVC Main (ADR-0003), AV1 Main `KEY_FRAME`-only (ADR-0005), VP9
  Profile 0 `KEY_FRAME` + general `INTER_FRAME` (ADR-0004) — all `VAEntrypointVLD`. Dispatched
  via `vaapi::VaapiVideoDecoder`, an enum over the four per-codec decoders (no `Box<dyn
  VideoDecoder>`); `LinuxVideoDecoder::open` picks a variant from `config.codec`.
- Scope: I and single-forward-reference P slices (real GOP/IPPP... decode) for H.264/HEVC,
  single slice per picture, progressive. Sliding-window DPB (`vaapi/dpb.rs`, ported from
  `vulkan/dpb.rs`), `RefPicList0[0]`-only reference — no reordering, no long-term refs, no
  weighted prediction, no B-slices, no multi-reference (all rejected honestly). VA-API (unlike
  Windows Media Foundation) requires the *application* to parse SPS/PPS/slice headers itself.
- CPU: NV12 readback via `Picture::create_image` (`vaCreateImage`+`vaGetImage`) — pitches/
  offsets stripped by `vaapi/nv12.rs` into a tightly packed buffer
- Zero-Copy: **implemented** (ADR-0006) — `VideoOutputPreference::ZeroCopyGpu` exports the
  decoded surface as a DMA-BUF via `Surface::export_prime()` (`vaExportSurfaceHandle`), returned
  as `mediaway_common::GpuBufferHandle::DmaBuf`. DPB slot recycling tracks outstanding exported
  handles (`Dpb::mark_outstanding`/`clear_outstanding`), refusing to recycle a slot a caller
  still holds. No downstream consumer exists anywhere in this workspace yet (no Linux
  `mediaway-wgpu` bridge) — this is the first DMA-BUF-aware code in the repo.
- Session shape: `Display`/`Config`/`Context`/`Surface` (safe `cros-libva` wrappers) +
  `Picture<S, T>` **typestate** enforcing `vaBeginPicture → vaRenderPicture → vaEndPicture →
  vaSyncSurface` ordering at compile time; pipeline creation is **lazy** (first SPS/header seen)
- ADR: [0001](../../../../crates/mediaway-decoder/adr/linux/0001-vaapi-h264-cpu-out.md) —
  H.264 binding choice, decode scope, **zero real-hardware verification** caveat
- ADR: [0002](../../../../crates/mediaway-decoder/adr/linux/0002-vaapi-h264-p-slice-dpb.md) —
  H.264 single-forward-reference P-slice decode, sliding-window DPB/POC ported from
  `vulkan/dpb.rs`. Found two real latent gaps in the Vulkan porting source itself
  (`pred_weight_table()`, `cabac_init_idc` unhandled) — rejected honestly rather than inherited.
- ADR: [0003 (HEVC)](../../../../crates/mediaway-decoder/adr/linux/0003-vaapi-hevc-p-slice-dpb.md) —
  HEVC single-forward-reference P-slice decode. Unlike H.264, **no hardware-verified porting
  source existed** (Vulkan's own HEVC decode is IDR-only) — fresh single-slot `HevcDpb`, reusing
  `vaapi/dpb.rs::derive_pic_order_cnt_msb` directly (ITU-T H.265 §8.3.1 shares H.264 §8.2.1.1's
  MSB/LSB formula), and a slice-header parser extended well past `vulkan::hevc_slice.rs`'s own
  stopping point (VA-API needs many explicit per-slice fields Vulkan's hardware parses directly).
- ADR: [0004 (VP9)](../../../../crates/mediaway-decoder/adr/linux/0004-vaapi-vp9-key-frame-and-inter-decode.md) —
  VP9 `KEY_FRAME` + **general** `INTER_FRAME` decode (no artificial reference-count restriction —
  `reference_frames[8]` is always fully populated regardless of active-reference count, per
  `FFmpeg`'s `vaapi_vp9.c`). VP9 entropy adaptation is driver-internal, reference model is a
  flat 8-slot array. Spec-derived `uncompressed_header()` parser copied verbatim from the real
  primary VP9 spec text. Confirmed VP9's `s(n)` (magnitude then separate sign bit) differs from
  AV1's `su(n)` (sign embedded in the same field).
- ADR: [0005 (AV1)](../../../../crates/mediaway-decoder/adr/linux/0005-vaapi-av1-key-frame-decode.md) —
  AV1 `KEY_FRAME`-only decode, single tile, Main profile, every optional coding tool rejected if
  signaled. No AV1 decode existed anywhere in this workspace to port from — the bitstream parser
  is spec-derived, cross-checked against `mediaway-encoder::windows::d3d12_video_encode::
  bitstream_av1`'s OBU writer (inverse direction). No `cros-libva` packed-header gap on the
  decode side (unlike its encoder sibling).
- ADR: [0006](../../../../crates/mediaway-decoder/adr/linux/0006-vaapi-dmabuf-zero-copy-output.md) —
  DMA-BUF Zero-Copy output, `mediaway_common::GpuBufferHandle::DmaBuf(Box<DmaBufDescriptor>)`
  (boxed — 4-5x bigger than any existing variant; drops `Copy` from the whole enum).
  `GpuBufferHandle: Copy`-removal blast radius across the whole workspace (Windows/Apple/
  Android/Web included): **zero** real call sites needed a fix.

All codecs/ADRs above: compile/clippy/test-verified on real WSL2 Linux (+ Windows workspace-wide
for the DMA-BUF work) only — **zero real VA-API hardware verification**, same caveat for every
ADR in this folder.

## ⚠️ Hardware verification status

Written and compile-verified on Linux (WSL2 Ubuntu 24.04, real `libva-dev` headers/bindgen
output) but **never run against a real VA-API display/driver**. The dev box is Windows (no
native VA-API); the available WSL2 instance has broken VA-API (`vainfo` segfaults) and only
software `llvmpipe` Vulkan. `Display::open()` honestly returns `DecodeError::Unsupported` in
that environment — this is the expected, documented outcome, not a bug. Treat every VA-API call
path and bitstream-parsing decision as unverified against a real driver until run on real Linux
+ VA-API hardware (Intel iHD / Mesa / AMD).

## Structural differences vs. Windows (WMF) CPU decode

| Windows | Linux | Note |
|---------|-------|------|
| Software decoder MFT parses the whole bitstream internally | Driver does entropy decode + reconstruction; **this crate** parses SPS/PPS/slice headers | VA-API's app/driver split, not MF's black-box MFT |
| `IMFTransform` (one object) | `Config` + `Context` (capability vs. bound session) | VA-API separates driver capability from session |
| Async event pump for HW MFTs | None needed | `vaSyncSurface` blocks; fully synchronous per picture |
| DX11 Zero-Copy decode out | DMA-BUF export (ADR-0006), no downstream consumer yet | Different mechanism, same absence of a real consumer |
