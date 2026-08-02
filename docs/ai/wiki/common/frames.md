# Frames and handles

In `mediaway-common`:

| Type | Role |
|------|------|
| `PixelFormat` / `SampleFormat` | Layout tags (NV12, BGRA, S16, …) |
| `VideoFrame` + `VideoFrameStorage` | CPU `Bytes` or `Gpu(GpuBufferHandle)` |
| `AudioFrame` | Interleaved PCM (`Bytes`; share for CPU ⚡) |
| `GpuBufferHandle` | Opaque platform tokens (DX11, Vulkan, WebGPU, …) |

**⚡:** GPU handle **or** shared CPU — [marks](../zero-copy/marks.md).  
Encode traits: [encode/scaffold](../encode/scaffold.md). Handles: [zero-copy/handles](../zero-copy/handles.md).
