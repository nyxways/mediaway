# mediaway-decoder — roadmap

**Facade** crate (traits). Platform backends: `mediaway-decoder-windows`, `mediaway-decoder-web`, …  
Packaging: [`docs/spec/crate-packaging.md`](../../../docs/spec/crate-packaging.md).  
Platform order: **Windows → Web → Linux → other**.  
Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 0 — Scaffold

- [x] Facade crate + `docs/` / `adr/`
- [x] ADR: decoder traits (align with encoder)
- [x] ADR: facade vs `mediaway-decoder-<platform>` boundary

### 1 — Windows

- [x] Add `mediaway-decoder-windows` workspace member
- [x] WMF H.264 decode (HW MFT, DX11 Zero-Copy out)
- [x] HEVC / AV1 / VP9 HW decode open (same DXGI path; MFT may be absent)
- [ ] CPU frame output path (`CpuFramesOk`)
- [ ] Round-trip with demuxer + encoder tests

### 2 — Web

- [ ] Add `mediaway-decoder-web`
- [ ] WebCodecs decode + `VideoFrame` / WebGPU interop

### 3 — Linux

- [ ] Add `mediaway-decoder-linux`
- [ ] VA-API / Vulkan Video decode
- [ ] `mediaway-decoder-vulkan` (portable, not OS-suffixed — see its own
      [ADR-0001](../../mediaway-decoder-vulkan/adr/0001-vulkan-video-decode.md)):
      ADR + scaffold only so far, H.264/HEVC/AV1 + general P/B-frame GOP
      scope, nothing implemented or hardware-verified yet

### 4 — Other

- [ ] `mediaway-decoder-apple` / `mediaway-decoder-android` as scheduled
