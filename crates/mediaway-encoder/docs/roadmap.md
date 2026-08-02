# mediaway-encoder — roadmap

**Facade** crate (traits). Platform backends: `mediaway-encoder-windows`, `mediaway-encoder-web`, …  
Packaging: [`docs/spec/crate-packaging.md`](../../../docs/spec/crate-packaging.md).  
Platform order: **Windows → Web → Linux → other**.  
Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 0 — Scaffold

- [x] Facade crate + `docs/` / `adr/`
- [x] ADR: `VideoEncoder` / `AudioEncoder` traits + streaming poll API
- [x] ADR: facade vs `mediaway-encoder-<platform>` boundary

### 1 — Windows

- [x] Add `mediaway-encoder-windows` workspace member
- [x] WMF H.264 encode (sync inbox MFT, CPU NV12 upload)
- [x] DX11 texture Zero-Copy push path (HW MFT + DXGI)
- [x] `auto` types in facade (ADR-0003); `AutoVideoEncodeConfig::new` (explicit size)
- [x] Windows `AutoVideoEncoder::open` / `WindowsVideoEncoder::open` (no free `open_*`)
- [x] GpuCopy path in `auto` (`DirectX12` → `D3d12SharedEncodeBridge`)
- [ ] Readback / SW paths in `auto` (policy bits recognized; no backend yet — honest `NoBackend` error)
- [x] WMF AAC encode
- [x] Integration smoke with `mediaway-container` + `mediaway-test-media`

### 1b — Umbrella (optional)

- [ ] `mediaway-codec` re-exports encoder (+ decoder when ready) for one-line app deps


### 2 — Web

- [x] Add `mediaway-encoder-web`
- [x] WebCodecs `VideoEncoder` / `AudioEncoder` (CPU path)
- [x] `GPUTexture` → encode Zero-Copy (via WebGPU-backed `OffscreenCanvas`; see caveat below)

### 3 — Linux

- [x] Add `mediaway-encoder-linux`
- [x] VA-API H.264 CPU-upload encode (`cros-libva`; Constrained Baseline, CQP, all-IDR) —
      **zero real-hardware verification**, see crate ADR-0001
- [ ] Vulkan Video encode (alternative/complement to VA-API)
- [ ] GPU buffer Zero-Copy where supported (DMA-BUF surface import)

### 4 — Other

- [ ] `mediaway-encoder-apple` / `mediaway-encoder-android` as scheduled
