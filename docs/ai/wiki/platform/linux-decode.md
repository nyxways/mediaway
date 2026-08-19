# Linux decode (VA-API)

- Module: `mediaway-decoder::linux`
- Bindings: [`cros-libva`](https://crates.io/crates/cros-libva) (BSD-3-Clause, `cfg(target_os =
  "linux")` dependency only — never pulled into non-Linux builds)
- Codec: H.264 baseline/main only (`vaapi/codec.rs` → `VAProfileH264ConstrainedBaseline` /
  `VAProfileH264Main`, `VAEntrypointVLD`)
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
- ADR: [0003](../../../../crates/mediaway-decoder/adr/linux/0003-vaapi-dmabuf-zero-copy-output.md)
  — **Implemented, WSL2 + Windows compile/clippy/test-verified**: DMA-BUF Zero-Copy output via
  `Surface::export_prime()` (`cros-libva` already wraps `vaExportSurfaceHandle` safely, new
  `vaapi/dmabuf.rs`). New `mediaway_common::GpuBufferHandle::DmaBuf(Box<DmaBufDescriptor>)`
  (boxed — the payload is 4-5x bigger than any existing variant; drops `Copy` from the whole
  enum). Re-introduces DPB `outstanding`/`mark_outstanding`/`clear_outstanding` tracking that
  ADR-0002 deliberately dropped (conditional on "no Zero-Copy handle exists yet" — no longer
  true), tied to a new `Pipeline::exported_fds: Vec<Option<OwnedFd>>` released at the top of the
  next `push_packet`/`poll_frame`/`flush`. Corrects the roadmap's stale "`GpuBufferHandle::
  Vulkan` interop" framing — DMA-BUF is a raw fd + plane layout, not a `VkImage`; no consumer
  (`mediaway-wgpu` Linux bridge) exists anywhere in this workspace yet, so this is genuinely the
  first DMA-BUF-aware code in the repo. `GpuBufferHandle: Copy`-removal blast radius across the
  whole workspace (Windows/Apple/Android/Web included): **zero** real call sites needed a fix —
  every existing construction site already builds a fresh value per call rather than
  implicit-copying one, exactly as the ADR's own (then-unverified) read predicted. **Zero real
  VA-API hardware verification** — same standing caveat as every VA-API path in this crate.

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
