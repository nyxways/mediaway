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
- [x] CPU frame output path (`CpuFramesOk`) for Windows H.264
- [x] H.264 CPU round-trip with demuxer + encoder tests
      (`crates/mediaway/tests/trim_and_splice_windows.rs`); the test skips
      gracefully when the host has no usable WMF encoder/decoder.

### 2 — Web

- [x] Add `mediaway-decoder-web`
- [x] WebCodecs decode + `VideoFrame` / WebGPU interop

### 3 — Linux

- [ ] Add `mediaway-decoder-linux`
- [ ] VA-API / Vulkan Video decode
- [ ] `mediaway-decoder-vulkan` (portable, not OS-suffixed — see its own
      [ADR-0001](../../mediaway-decoder-vulkan/adr/0001-vulkan-video-decode.md)):
      ADR + scaffold only so far, H.264/HEVC/AV1 + general P/B-frame GOP
      scope, nothing implemented or hardware-verified yet

### 4 — Other

- [x] `android` module: NDK `AMediaCodec` H.264 CPU NV12 decode, general GOP —
      zero compile/runtime verification (no NDK/device in dev env); see
      [adr/android/0001](../adr/android/0001-ndk-amediacodec-h264-cpu-out.md)
- [x] `apple` module: `VTDecompressionSession` H.264 general-GOP CPU-output decode
      (`src/apple/`, one SPS + one PPS, 4-byte AVCC length size only) — compiles/lints on this
      Windows host (the real `objc2-*`-calling code is `cfg`-gated to Apple targets; pure
      `codec.rs` tick/NV12 helpers are host-testable and covered by real unit tests), **zero
      compile verification of the Apple-only code path itself** (no Apple SDK in this dev
      environment) — not wired into `auto`/`capability` yet; see
      [adr/apple/0001](../adr/apple/0001-videotoolbox-h264-cpu-out.md)
