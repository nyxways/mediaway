# mediaway-common — roadmap

Platform order (workspace): **Windows → Web → Linux → other**.  
Index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 0 — Types bootstrap

- [x] `Rational`
- [x] `PixelFormat`, `SampleFormat`
- [x] `GpuBufferHandle` enum (all platform variants declared; impls elsewhere)
- [x] Shared `Packet` / `VideoFrame` / `AudioFrame` input types used by encoder/decoder/mux

### 1 — Windows-ready surface

- [x] Types needed for WMF + DX11 Zero-Copy encode path (handle + NV12 frame tags)
- [ ] Docs for handle ownership / lifetime contracts (filled when MF session lands)
- [ ] Note: `mediaway-wgpu` adapter is a **separate** crate ([`gpu-interop.md`](../../../docs/spec/gpu-interop.md))

### 2 — Web-ready surface

- [ ] `WebGpu` handle variant contracts for WASM

### 3 — Linux-ready surface

- [ ] Vulkan / VA-API related handle fields as required by encoder/decoder

### 4 — Other platforms

- [ ] Metal / Android surface handle completeness
