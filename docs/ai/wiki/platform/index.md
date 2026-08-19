# Platform

| Doc | Summary |
|-----|---------|
| [order](order.md) | Build order: Windows → Web → Linux → other |
| [windows-encode](windows-encode.md) | WMF + DX11 encoder backend status |
| [windows-encode-d3d12](windows-encode-d3d12.md) | D3D12 native Video Encode API: H.264/HEVC/AV1 all-intra, GOP, row-based intra refresh — hardware findings |
| [windows-encode-gpu-input](windows-encode-gpu-input.md) | D3D12 encoder GPU-input Zero-Copy design (wgpu + native capture callers) — Proposed, no code |
| [windows-decode](windows-decode.md) | WMF + DX11 decoder Zero-Copy out; D3D12 native decode (H.264 implemented, **paused** on a real GPU-TDR hang) |
| [windows-decode-d3d12-hevc](windows-decode-d3d12-hevc.md) | D3D12 native HEVC decode (ADR-0004) — **implemented, sans-io-verified only**; zero real GPU hardware run, deliberately |
| [windows-decode-d3d12-av1](windows-decode-d3d12-av1.md) | D3D12 native AV1 decode (ADR-0005) — **implemented, sans-io-verified only**, `KEY_FRAME`-only; zero real GPU hardware run, deliberately; open bitstream-source question for any future hardware attempt |
| [web-encode](web-encode.md) | WebCodecs + WASM mux (browser) |
| [linux-encode](linux-encode.md) | VA-API H.264 CPU-upload encode backend; real P-frame GOP implemented (ADR-0002); zero HW verification caveat |
| [linux-decode](linux-decode.md) | VA-API H.264 CPU-output decode backend — real GOP (IPPP...) decode via a sliding-window DPB, single-forward-reference P-slices (ADR-0002); zero HW verification caveat |
| [android-encode](android-encode.md) | NDK `AMediaCodec` H.264 CPU-upload encode — implemented, zero compile verification until CI |
| [android-decode](android-decode.md) | NDK `AMediaCodec` H.264 CPU-output decode — **implemented, zero compile/runtime verification**; general-GOP (not IDR-only), decoder-chosen output layout |
| [apple-encode](apple-encode.md) | `VideoToolbox` `VTCompressionSession` H.264 CPU-upload encode — implemented, zero compile verification until CI |
| [apple-decode](apple-decode.md) | `VideoToolbox` `VTDecompressionSession` H.264 general-GOP CPU-output decode — ADR only, zero compile verification |
| [vulkan-encode](vulkan-encode.md) | Vulkan Video H.264 encode: capability probe + real minimal session, hardware-verified |
| [vulkan-decode](vulkan-decode.md) | Vulkan Video decode: H.264 **hardware-verified** (first general-GOP backend in workspace); HEVC IDR decode **hardware-verified** too (P/B deferred) |
| [support-matrix](support-matrix.md) | Link to human OS/GPU/CPU codec tables |
