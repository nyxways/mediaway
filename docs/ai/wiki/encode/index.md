# Encode

| Doc | Summary |
|-----|---------|
| [scaffold](scaffold.md) | Facade traits + `auto`; Windows WMF/DX11 |
| [auto](auto.md) | High-level auto encode · path classes · `mediaway-codec` |
| [backend-preference](backend-preference.md) | Auto/Os/Cpu/Gpu(Graphics\|VendorHw)/Sw · README axes |
| [web-gpu-frame](web-gpu-frame.md) | Web: WebGPU-backed `VideoFrame` — no `GPUTexture` ctor, canvas path, honesty label |
| [web-real-chrome-bugs](web-real-chrome-bugs.md) | Web: 3 real bugs found only via real Chrome + CDP (arg order, description, AAC flush) |
| [web-opus-design](web-opus-design.md) | Web: codec-parameterized audio smoke surface (`is_webcodecs_audio_codec_supported`, `encode_audio_buffer`), Opus exercised alongside AAC — wasm32 compile-verified only; no `OpusEncoderConfig`; `iso-bmff` mislabels Opus as `mp4a` |
| [../platform/linux-encode](../platform/linux-encode.md) | Linux: VA-API H.264 CPU-upload (`cros-libva`); zero HW verification caveat |
| [linux-h264-gop](linux-h264-gop.md) | Linux VA-API H.264 P-frame GOP: `GopState` port, capability gate — **implemented**, zero HW verification |
| [../platform/android-encode](../platform/android-encode.md) | Android: NDK `AMediaCodec` H.264 CPU-upload via `ndk` crate — implemented, zero compile verification until CI |
| [../platform/apple-encode](../platform/apple-encode.md) | Apple: `VideoToolbox` `VTCompressionSession` H.264 CPU-upload via `objc2-*` — implemented, zero compile verification until CI |
| [../platform/vulkan-encode](../platform/vulkan-encode.md) | Vulkan: probe/H.264/HEVC/AV1 + GOP (H.264+HEVC) / CBR (H.264 only) rate control — hardware-verified |
| [vulkan-h264-gop](vulkan-h264-gop.md) | Vulkan H.264 GOP/CBR detail: `GopState`/DPB ring, barrier fix, FFI chaining |
| [vulkan-hevc-gop](vulkan-hevc-gop.md) | Vulkan HEVC GOP detail: separate `GopState`, picture-embedded short-term RPS, CBR stays H.264-only |
| [vulkan-av1-gop](vulkan-av1-gop.md) | Vulkan AV1 GOP detail: `order_hint`-keyed `GopState`, implemented but **unverifiable** (base encode already driver-broken) |
| [../license/sw-opus](../license/sw-opus.md) | `mediaway-sw::opus` — real Opus encode via `unsafe-libopus`; closes the only-platform-with-zero-Opus-encode gap |

Codec matrices: root [`README.md`](../../../../README.md#codec-support) · wiki pointer [support-matrix](../platform/support-matrix.md).  
Platform order: Windows → Web → Linux → other — [`docs/roadmap.md`](../../../roadmap.md).  
Packaging: [crate-packaging](../meta/crate-packaging.md). Traits: crate ADR-0001.
