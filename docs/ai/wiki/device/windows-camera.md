# Windows camera capture (Media Foundation)

- Module: `mediaway-device::windows_camera`
- API: `WindowsCameraCapture::open` — `IMFSourceReader` via `MFEnumDeviceSources`
- Output: CPU-only (`VideoFrameStorage::Cpu`); NV12 or BGRA8 negotiated, preferring
  whichever the camera exposes natively, else Media Foundation's video-processor
  conversion (most USB webcams are MJPG/YUY2-native, not NV12/RGB32)
- `device` index = `MFEnumDeviceSources` enumeration ordinal — see
  [camera-device-handle](camera-device-handle.md)
- Hardware-verified: captured real 1920x1080 frames from a physical "WeVO WV-1080"
  USB webcam on the dev machine
- **Not** wired into `mediaway::platform` (no `Camera` cross-platform
  dispatcher exists yet) — only the direct crate API is reachable today
- No DX11 Zero-Copy path yet (unlike screen/window capture) — CPU copy is the only mode
- ADR: [windows_camera source](../../../../crates/mediaway-device/src/windows_camera/capture.rs)
  (module docs); no dedicated ADR file yet
