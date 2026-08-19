# Linux encode (VA-API)

- Module: `mediaway-encoder::linux`
- Bindings: [`cros-libva`](https://crates.io/crates/cros-libva) (BSD-3-Clause, `cfg(target_os =
  "linux")` dependency only — never pulled into non-Linux builds)
- Codec: H.264 Constrained Baseline (`VAProfileH264ConstrainedBaseline`, `VAEntrypointEncSlice`),
  HEVC Main (`VAProfileHEVCMain`, ADR-0003, same entrypoint), and VP9 Profile 0
  (`VAProfileVP9Profile0`, ADR-0004, 3-step entrypoint probe `EncSlice` → `EncPicture` →
  `EncSliceLP`) — dispatched via `vaapi::VaapiVideoEncoder`, an enum over the three per-codec
  encoders (no `Box<dyn VideoEncoder>`); `LinuxVideoEncoder::open` picks a variant from
  `config.codec`. AV1 encode is designed but **blocked** (see ADR-0003 below).
- CPU: `upload_cpu_nv12` — `Image::create_from` (`vaCreateImage`+`vaGetImage`) + memcpy +
  `vaPutImage` on drop
- Zero-Copy: **not implemented** — `VideoInputPreference::ZeroCopyGpu` returns `Unsupported`
  (deferred: DMA-BUF surface import, `VASurfaceAttribExternalBuffers`)
- Rate control: `VA_RC_CQP` fixed QP only; **every pushed frame is an independent IDR by
  default** (`gop_size <= 1`) — real single-forward-reference P-frame GOP (H.264/HEVC/VP9) is
  **implemented**, capability-gated where applicable
- Session shape: `Display`/`Config`/`Context`/`Surface` (safe `cros-libva` wrappers) +
  `Picture<S, T>` **typestate** enforcing `vaBeginPicture → vaRenderPicture → vaEndPicture →
  vaSyncSurface` ordering at compile time
- ADR: [0001](../../../../crates/mediaway-encoder/adr/linux/0001-vaapi-cros-libva-h264-cpu-upload.md)
  — binding choice, scope, **zero real-hardware verification** caveat
- ADR: [0002](../../../../crates/mediaway-encoder/adr/linux/0002-vaapi-h264-p-frame-gop.md) —
  single-forward-reference P-frame GOP, ported `vulkan::h264_gop::GopState` verbatim. Real
  cross-check finding: this backend's `pic_order_cnt_type = 2` output is not decodable by this
  workspace's own `mediaway-decoder::linux::vaapi` (only accepts `pic_order_cnt_type == 0`) — a
  real, deliberately-unresolved cross-crate interop gap. See
  [linux-h264-gop](../encode/linux-h264-gop.md).
- ADR: [0003 (HEVC)](../../../../crates/mediaway-encoder/adr/linux/0003-vaapi-hevc-p-frame-gop.md) —
  HEVC single-forward-reference P-frame GOP, porting `vulkan::hevc_gop::GopState` verbatim (no
  `frame_num`/`idr_pic_id`, `poc` never wraps) but **not** `vulkan::hevc_params.rs`'s
  `StdVideoH265*` construction — `EncSequenceParameterBufferHEVC` has no
  `log2_max_pic_order_cnt_lsb_minus4`/`max_num_ref_frames` field at all (the driver synthesizes
  HEVC parameter sets itself, unlike Vulkan Video), so `vaapi/hevc.rs` builds those fresh,
  grounded in FFmpeg's real `vaapi_encode_h265.c`.
- ADR: [0003 (AV1)](../../../../crates/mediaway-encoder/adr/linux/0003-vaapi-av1-key-frame-and-inter-gop.md) —
  **Design only, blocked**: AV1 `KEY_FRAME` baseline + single-forward-reference `INTER_FRAME`
  GOP. Cannot be implemented against `cros-libva` 0.0.13 as pinned: real AV1 VA-API encode needs
  the app to submit a packed `frame_header_obu()` bitstream buffer, a `BufferType` variant
  `cros-libva` does not wrap (and this crate's `#![forbid(unsafe_code)]` rules out a raw-FFI
  workaround) — needs a `cros-libva` fork/upstream PR first.
- ADR: [0004 (VP9)](../../../../crates/mediaway-encoder/adr/linux/0004-vaapi-vp9-key-frame-and-inter-gop.md) —
  VP9 `KEY_FRAME` baseline + single-forward-reference `INTER_FRAME` GOP (new `vp9_gop.rs` 2-slot
  physical ping-pong state machine, cross-checked against FFmpeg's real
  `vaapi_encode_vp9_init_picture_params`). Real vendored `cros-libva` VP9 encode structs are
  plain C-struct field bags — the driver synthesizes VP9's own header bytes (confirmed:
  `vaapi_encode_vp9.c`'s comment "the one usable driver (i965) can write its own headers").
  Entrypoint probe is a real 3-step ladder matching FFmpeg's own generic probe order — the ADR's
  original design was a 2-step ladder, corrected once FFmpeg's source showed
  `VAEntrypointEncPicture` is what VP9 actually needs (no slice concept). **Real caveat, not a
  code bug**: VP9 VA-API *encode* driver support is narrow (i965 only) — meaningfully less
  universal than VP9 decode.

All implemented codecs above: compile/clippy/test-verified on real WSL2 Linux only — **zero
real-hardware verification**, same caveat for every ADR in this folder.

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
