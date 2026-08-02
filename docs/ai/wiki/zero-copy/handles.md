# GpuBufferHandle / shared CPU

GPU resource handles for Zero-Copy encode/decode. Enum in `mediaway-common`; backends cast opaque `usize` fields.

**CPU↔CPU Zero-Copy** (audio, mapped PCM, packet `Bytes` share) does **not** use `GpuBufferHandle` — it uses shared/borrowed CPU buffers without a payload copy. Same README **⚡** mark when earned; current WASAPI still copies → 🆗.

Framework interop: [`docs/spec/gpu-interop.md`](../../../spec/gpu-interop.md).

| Variant | Platform | Native type |
|---------|----------|-------------|
| `DirectX11` | Windows | `ID3D11Texture2D*` (+ subresource) |
| `DirectX12` | Windows | `ID3D12Resource*` |
| `DirectXShared` | Windows | shared `HANDLE` |
| `Metal` | macOS/iOS | `CVPixelBuffer` / IOSurface token |
| `AndroidSurface` | Android | `AHardwareBuffer*` |
| `Vulkan` | Linux/Win/Android | `VkImage` + memory cookie |
| `WebGpu` | WASM / browser | host `texture_id` |

**wgpu (Rust):** export via planned `mediaway-wgpu` to `DirectX*` / `Vulkan` / `Metal`.

Ownership / fences: Windows encode ADR-0003 · decode ADR-0001 · capture ADR-0001 / ADR-0004.
