# Linux encode (VA-API)

- Module: `mediaway-encoder::linux`
- Bindings: [`cros-libva`](https://crates.io/crates/cros-libva) (BSD-3-Clause, `cfg(target_os =
  "linux")` dependency only — never pulled into non-Linux builds)
- Codec: H.264 Constrained Baseline only (`vaapi/codec.rs` → `VAProfileH264ConstrainedBaseline`,
  `VAEntrypointEncSlice`)
- CPU: `upload_cpu_nv12` — `Image::create_from` (`vaCreateImage`+`vaGetImage`) + memcpy +
  `vaPutImage` on drop
- Zero-Copy: **not implemented** — `VideoInputPreference::ZeroCopyGpu` returns `Unsupported`
  (deferred: DMA-BUF surface import, `VASurfaceAttribExternalBuffers`)
- Rate control: `VA_RC_CQP` fixed QP only; **every pushed frame is an independent IDR by
  default** (`gop_size <= 1`) — real single-forward-reference P-frame GOP (ADR-0002) is
  **implemented**, capability-gated on `VAConfigAttribEncMaxRefFrames`
- Session shape: `Display`/`Config`/`Context`/`Surface` (safe `cros-libva` wrappers) +
  `Picture<S, T>` **typestate** enforcing `vaBeginPicture → vaRenderPicture → vaEndPicture →
  vaSyncSurface` ordering at compile time
- ADR: [0001](../../../../crates/mediaway-encoder/adr/linux/0001-vaapi-cros-libva-h264-cpu-upload.md)
  — binding choice, scope, **zero real-hardware verification** caveat
- ADR: [0002](../../../../crates/mediaway-encoder/adr/linux/0002-vaapi-h264-p-frame-gop.md) —
  **Implemented**: single-forward-reference P-frame GOP, ported `mediaway-encoder::vulkan::
  h264_gop::GopState` (already GPU-API-agnostic, needed no adaptation) into a new sans-io
  `vaapi/gop.rs`; wires `VideoEncoderConfig::gop_size` into this backend for the first time,
  capability-gated on `VAConfigAttribEncMaxRefFrames`. See
  [linux-h264-gop](../encode/linux-h264-gop.md) for detail, including a real cross-check finding:
  this backend's `pic_order_cnt_type = 2` output is not decodable by this workspace's own
  `mediaway-decoder::linux::vaapi` (which only accepts `pic_order_cnt_type == 0`) — a real,
  pre-existing, deliberately-unresolved cross-crate interop gap.

## ⚠️ Hardware verification status

Written and compile-verified on Linux (WSL2 Ubuntu 24.04, real `libva-dev` 1.20.0 headers/
bindgen output) but **never run against a real VA-API display/driver**. The dev box is Windows
(no native VA-API); the available WSL2 instance has broken VA-API (`vainfo` segfaults) and only
software `llvmpipe` Vulkan. `Display::open()` honestly returns `EncodeError::Backend` in that
environment — this is the expected, documented outcome, not a bug. Treat every VA-API call path
in this crate as unverified until run on real Linux + VA-API hardware (Intel iHD / Mesa / AMD).

## Structural differences vs. Windows (WMF)

| Windows | Linux | Note |
|---------|-------|------|
| `IMFTransform` (one object) | `Config` + `Context` (capability vs. bound session) | VA-API separates driver capability from session |
| Async event pump for HW MFTs | None needed | `vaSyncSurface` blocks; fully synchronous per frame |
| `ProcessOutput` buffer loop | `EncCodedBuffer` + `MappedCodedBuffer` segments | Mapped buffer, not a pull queue |
| DX11 Zero-Copy | Not implemented this stage | See ADR-0001 § Scope |
