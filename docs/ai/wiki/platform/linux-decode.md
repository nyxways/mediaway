# Linux decode (VA-API)

- Crate: `mediaway-decoder-linux`
- Bindings: [`cros-libva`](https://crates.io/crates/cros-libva) (BSD-3-Clause, `cfg(target_os =
  "linux")` dependency only — never pulled into non-Linux builds)
- Codec: H.264 baseline/main only (`vaapi/codec.rs` → `VAProfileH264ConstrainedBaseline` /
  `VAProfileH264Main`, `VAEntrypointVLD`)
- Scope: **IDR pictures only**, single slice per picture, `pic_order_cnt_type == 0`,
  progressive — no DPB / reference picture management at all (an IDR I-slice references
  nothing). Own H.264 SPS/PPS/slice-header parser (`vaapi/sps.rs`/`pps.rs`/`slice.rs`) reuses
  [`mediaway_sw::h264`](../../../crates/mediaway-sw/docs/roadmap.md)'s `BitReader` + Annex-B
  NAL splitting — VA-API (unlike Windows Media Foundation) requires the *application* to
  parse SPS/PPS/slice headers itself.
- CPU: NV12 readback via `Picture::create_image` (`vaCreateImage`+`vaGetImage`) — pitches/
  offsets stripped by `vaapi/nv12.rs` into a tightly packed buffer (same layout as
  `mediaway-decoder-windows`'s `wmf/cpu.rs`)
- Zero-Copy: **not implemented** — `VideoOutputPreference::ZeroCopyGpu` returns `Unsupported`
  (deferred: DMA-BUF surface export, `vaExportSurfaceHandle`)
- Session shape: `Display`/`Config`/`Context`/`Surface` (safe `cros-libva` wrappers) +
  `Picture<S, T>` **typestate** enforcing `vaBeginPicture → vaRenderPicture → vaEndPicture →
  vaSyncSurface` ordering at compile time; pipeline creation is **lazy** (first SPS seen),
  since `open()` cannot know profile/coded resolution before that
- ADR: [0001](../../../crates/mediaway-decoder-linux/adr/0001-vaapi-h264-cpu-out.md) —
  binding choice, decode scope, **zero real-hardware verification** caveat

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
