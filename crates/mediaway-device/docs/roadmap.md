# mediaway-device — roadmap

**Facade** crate (traits). Platform backends: `mediaway-device-windows`, `mediaway-device-web`, …  
Packaging: [`docs/spec/crate-packaging.md`](../../../docs/spec/crate-packaging.md).  
Platform order: **Windows → Web → Linux → other**.  
Starts after encode MVP exists on a platform (capture → encode).  
Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 0 — Scaffold

- [x] Facade crate + `docs/` / `adr/`
- [x] ADR: device/capture traits + `GpuBufferHandle` output
- [x] ADR: how facade features select backends (platform crates; no default platform dep)
- [x] [ADR-0003](../adr/0003-capability-and-permission-probe.md): `DeviceKind` /
      `Support` / `Unavailable` / `PermissionState` — live capability + OS
      permission probing, separate from opening a session. Backends:
      `mediaway-device-windows`/`-linux` `capabilities.rs`; dispatch:
      `mediaway::platform::{device_support, request_device_permission}`.
- [x] [ADR-0004](../adr/0004-audio-playback-traits.md): `AudioPlayback` /
      `AudioPlaybackConfig` / `PlaybackError` — push-model playback trait
      (`write_frame`), mirroring `AudioCapture` in the opposite data
      direction. Backend: `mediaway-device-windows::WindowsWasapiPlayback`.
- [x] [ADR-0005](../adr/0005-device-selection.md) (Accepted): `DeviceId` /
      `Select` / `DeviceInfo` — replaces raw `device_index`/`device`/
      `output_index` fields across `AudioCaptureSource`, `AudioPlaybackConfig`,
      `CaptureSource`. `DeviceHotplug`/`DeviceEvent` vocabulary types added
      (trait declaration only, no backend). `CaptureError::DeviceLost` /
      `PlaybackError::DeviceLost` added. Backend: `mediaway-device-windows`
      `enumerate` (Microphone/Loopback/Camera/Screen, hardware-verified) +
      `DeviceLost` wiring into `wasapi.rs`/`wasapi_playback.rs`'s worker
      loops. **Still pending**: `WindowsDeviceHotplug` (`IMMNotificationClient`)
      backend implementation — separate follow-up task.
- [x] [ADR-0006](../adr/0006-capture-once-screenshot.md): single-shot
      zero-copy frame — `VideoCapture::capture_next_frame_blocking` (default
      trait method) + `capture_video_once` (generic facade composition),
      same `VideoFrame`/`GpuBufferHandle` as continuous capture. Implemented
      in `desktop::video` and `camera::capture`.

### 1 — Windows

- [x] Add `mediaway-device-windows` workspace member
- [x] Screen capture (DXGI Desktop Duplication Zero-Copy)
- [x] Window capture (WGC / `WindowsWindowCapture`)
- [x] WASAPI mic + system + process loopback (`WindowsWasapiCapture`)
- [x] WASAPI shared-mode render playback (`WindowsWasapiPlayback`,
      [ADR-0004](../adr/0004-audio-playback-traits.md))
- [x] Camera (Media Foundation, `windows_camera` module) — hardware-verified
      against a real USB webcam; see `docs/windows/roadmap.md` Stage 4
- [ ] Named BGRA→NV12 GpuCopy only if HW MFT rejects ARGB32

### 2 — Web

- [x] Add `mediaway-device-web`
- [x] `getUserMedia` / `getDisplayMedia` bindings (picker only; no programmatic selection)

### 3 — Linux

- [ ] Add `mediaway-device-linux`
- [ ] PipeWire / V4L2 / Portal capture paths

### 4 — Other

- [ ] `mediaway-device-apple` / `mediaway-device-android` as scheduled
