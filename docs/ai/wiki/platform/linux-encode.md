# Linux encode (VA-API)

- Module: `mediaway-encoder::linux`
- Bindings: [`cros-libva`](https://crates.io/crates/cros-libva) (BSD-3-Clause, `cfg(target_os =
  "linux")` dependency only — never pulled into non-Linux builds)
- Codec: H.264 Constrained Baseline (`vaapi/codec.rs` → `VAProfileH264ConstrainedBaseline`,
  `VAEntrypointEncSlice`) and VP9 Profile 0 (`VAProfileVP9Profile0`, 3-step entrypoint probe
  `EncSlice` → `EncPicture` → `EncSliceLP`) — dispatched via `vaapi::VaapiVideoEncoder`, an enum
  over the two per-codec encoders (no `Box<dyn VideoEncoder>`, this backend's first multi-codec
  **encoder** dispatch enum)
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
- ADR: [0003](../../../../crates/mediaway-encoder/adr/linux/0003-vaapi-av1-key-frame-and-inter-gop.md) —
  **Design only, blocked**: AV1 `KEY_FRAME` baseline + single-forward-reference `INTER_FRAME` GOP,
  porting the OBU byte-writer from `mediaway-encoder::windows::d3d12_video_encode::bitstream_av1`
  and the GOP state machine from `mediaway-encoder::vulkan::av1_gop`. Cannot be implemented against
  `cros-libva` 0.0.13 as pinned: real AV1 VA-API encode needs the app to submit a packed
  `frame_header_obu()` bitstream buffer, a `BufferType` variant `cros-libva` does not wrap (and
  this crate's own `#![forbid(unsafe_code)]` rules out a local raw-FFI workaround) — needs a
  `cros-libva` fork/upstream PR first. There is **no VA-API HEVC ADR** in this workspace (HEVC
  exists only on the Vulkan backend) — an earlier task brief's premise to the contrary was
  checked and found false; see the ADR's own § Note.
- ADR: [0004](../../../../crates/mediaway-encoder/adr/linux/0004-vaapi-vp9-key-frame-and-inter-gop.md) —
  **Implemented, WSL2 compile+clippy+test-verified**: VP9 `KEY_FRAME` baseline +
  single-forward-reference `INTER_FRAME` GOP (`vaapi/vp9.rs` + new `vaapi/vp9_gop.rs` 2-slot
  physical ping-pong state machine, cross-checked — not ported, no in-workspace VP9 GOP
  precedent existed — against `FFmpeg`'s real `vaapi_encode_vp9_init_picture_params`, quoted
  verbatim in the ADR). Real vendored `cros-libva` VP9 encode structs
  (`EncSequenceParameterBufferVP9`/`EncPictureParameterBufferVP9`) are plain C-struct field bags —
  the driver synthesizes VP9's own header bytes, confirmed by `FFmpeg`'s own
  `vaapi_encode_vp9.c` comment: "the one usable driver (i965) can write its own headers." The
  missing `cros-libva::EncSliceParameter::VP9` variant is **not** a gap (real libva has no such
  buffer for VP9 encode either) — confirmed, VP9 submits exactly two buffers per frame
  (`EncSequenceParameter`, once per session, + `EncPictureParameter`), no third slice-shaped
  buffer at all. Entrypoint probe is a real 3-step ladder (`EncSlice` → `EncPicture` →
  `EncSliceLP`) per `FFmpeg`'s own generic `vaapi_encode.c` probe order — the ADR's own original
  design was a 2-step ladder, corrected via its addendum after `FFmpeg`'s real generic source
  showed `VAEntrypointEncPicture` is what VP9 actually uses (VP9 has no slice concept at all).
  **Real caveat, not a code bug**: VP9 VA-API **encode** driver support is narrow (i965 only,
  per `FFmpeg`'s own comment) — meaningfully less universal than VP9 *decode*; this is a
  compile/test-verified-only backend addition, not an expected-to-work-on-most-hardware one.
  **Zero real-hardware verification**, same caveat as every ADR in this folder.

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
