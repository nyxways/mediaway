# Platform

| Doc | Summary |
|-----|---------|
| [order](order.md) | Build order: Windows → Web → Linux → other |
| [windows-encode](windows-encode.md) | WMF + DX11 encoder backend status |
| [windows-encode-d3d12](windows-encode-d3d12.md) | D3D12 native Video Encode API: H.264/HEVC/AV1 all-intra, GOP, row-based intra refresh — hardware findings |
| [windows-decode](windows-decode.md) | WMF + DX11 decoder Zero-Copy out; D3D12 native decode (H.264 implemented, **paused** on a real GPU-TDR hang) |
| [web-encode](web-encode.md) | WebCodecs + WASM mux (browser) |
| [linux-encode](linux-encode.md) | VA-API H.264 CPU-upload encode backend; zero HW verification caveat |
| [linux-decode](linux-decode.md) | VA-API H.264 CPU-output decode backend (IDR-only); zero HW verification caveat |
| [android-encode](android-encode.md) | NDK `AMediaCodec` H.264 CPU-upload encode — implemented, zero compile verification until CI |
| [apple-encode](apple-encode.md) | `VideoToolbox` `VTCompressionSession` H.264 CPU-upload encode — implemented, zero compile verification until CI |
| [vulkan-encode](vulkan-encode.md) | Vulkan Video H.264 encode: capability probe + real minimal session, hardware-verified |
| [vulkan-decode](vulkan-decode.md) | Vulkan Video decode: H.264 **hardware-verified** (first general-GOP backend in workspace); HEVC IDR decode **hardware-verified** too (P/B deferred) |
| [support-matrix](support-matrix.md) | Link to human OS/GPU/CPU codec tables |
