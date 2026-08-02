# Platform

| Doc | Summary |
|-----|---------|
| [order](order.md) | Build order: Windows → Web → Linux → other |
| [windows-encode](windows-encode.md) | WMF + DX11 encoder backend status |
| [windows-decode](windows-decode.md) | WMF + DX11 decoder Zero-Copy out; D3D12 native decode (H.264 implemented, **paused** on a real GPU-TDR hang) |
| [web-encode](web-encode.md) | WebCodecs + WASM mux (browser) |
| [linux-encode](linux-encode.md) | VA-API H.264 CPU-upload encode backend; zero HW verification caveat |
| [linux-decode](linux-decode.md) | VA-API H.264 CPU-output decode backend (IDR-only); zero HW verification caveat |
| [vulkan-encode](vulkan-encode.md) | Vulkan Video H.264 encode: capability probe + real minimal session, hardware-verified |
| [vulkan-decode](vulkan-decode.md) | Vulkan Video decode: H.264 **hardware-verified** (first general-GOP backend in workspace); HEVC sans-io real, GPU path still all-zero |
| [support-matrix](support-matrix.md) | Link to human OS/GPU/CPU codec tables |
