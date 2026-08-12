# Device

| Doc | Summary |
|-----|---------|
| [scaffold](scaffold.md) | Facade `mediaway-device`; backends `mediaway-device::windows` / `mediaway-device::web` / … |
| [windows-capture](windows-capture.md) | DXGI DDA screen Zero-Copy |
| [windows-window](windows-window.md) | WGC single-window capture (≠ screen) |
| [windows-audio](windows-audio.md) | WASAPI mic / loopback capture + shared-mode render playback (CPU ⚡ later) |
| [windows-camera](windows-camera.md) | Media Foundation camera capture — real, hardware-verified, CPU-only |
| [camera-device-handle](camera-device-handle.md) | Why `CaptureSource::Camera.device` stays untyped (ADR-0013) — resolved as an enumeration index, superseded by [selection](selection.md) |
| [selection](selection.md) | `DeviceId` / `Select` / `DeviceInfo` (ADR-0005, Accepted) — replaces raw device indices; Windows `enumerate` + `DeviceLost` + `WindowsDeviceHotplug` (Microphone/Loopback) all done |
| [capture-once](capture-once.md) | Single-shot capture (ADR-0006, Accepted) — `capture_next_frame_blocking` + `capture_video_once`; the latter refuses GPU-backed frames (dangling-handle fix), see `ffi-c-abi` |
| [linux-capture](linux-capture.md) | `xdg-desktop-portal` `ScreenCast` + PipeWire screen capture (CPU copy, unverified this session) |
| [linux-camera](linux-camera.md) | V4L2 camera capture (`v4l` crate, no `unsafe`, unverified this session) |
| [linux-window](linux-window.md) | Portal `SourceType::Window` capture (shares screen-capture plumbing, unverified) |
| [linux-mic](linux-mic.md) | Direct PipeWire microphone capture (no portal, unverified this session) |
| [android-capture](android-capture.md) | Camera (Camera2 NDK) + mic (AAudio) + screen (`MediaProjection` + JNI) — implemented, zero compile verification until CI |
| [apple-capture](apple-capture.md) | Camera (`AVCaptureSession`) + mic (`AVAudioEngine`) + screen (`ScreenCaptureKit`/`ReplayKit`, incl. iOS Broadcast Extension sink) — implemented, zero compile verification until CI |
| [capabilities](capabilities.md) | `DeviceKind`/`Support`/`PermissionState` — live capability + OS permission probe, separate from opening a session |
| [ffi-c-abi](ffi-c-abi.md) | `mediaway-ffi` — C ABI over Camera + Screen (GPU handle, ADR-0003) video + Microphone/Loopback/ProcessLoopback audio; Window deferred |
| [gpu-device-factory-ffi](gpu-device-factory-ffi.md) | `mediaway-ffi` — GPU adapter enumeration + device create/close C ABI (ADR-0007); closes the "no C caller can reach Screen capture" gap |
| [hotplug-ffi](hotplug-ffi.md) | `mediaway-ffi` hotplug — callback + poll event delivery (ADR-0002, Accepted; real Windows `open`/`poll_event` wired in and hardware-confirmed; `close()` has an unresolved real-hardware crash, `mediaway-device::windows` follow-up) |

Support matrix: root [README § Device](../../../../README.md#device).

Packaging: [crate-packaging](../meta/crate-packaging.md).

Audio *enhancement* (echo cancel / noise suppress / gain control / VAD) is a
separate capability, not part of this facade — see
[audio/apm](../audio/apm.md) (`mediaway-sw::apm`, Proposed). It consumes
`AudioCapture::poll_frame()`'s output.
