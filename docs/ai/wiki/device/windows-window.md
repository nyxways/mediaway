# Windows window capture (WGC)

- Crate: `mediaway-device-windows`
- API: `WindowsWindowCapture::open` — WinRT Graphics Capture (`CreateForWindow`)
- Source: `CaptureSource::Window` / `VideoCaptureConfig::window` (≠ Screen)
- Output: `Bgra8` + `DirectX11` Zero-Copy; hold frame until `release_frame`
- Audio pair: `AudioCaptureSource::ProcessLoopback` for same-app sound
- Overlay: `exclude_window_from_capture` so HUD is omitted from DDA/WGC
- Resize: `poll_frame` detects a `Frame.ContentSize` mismatch and calls
  `Direct3D11CaptureFramePool::Recreate` at the new size before delivering the frame
  (previously every frame after a resize was silently skipped forever)
- ADR: [0004](../../../crates/mediaway-device-windows/adr/0004-wgc-window-capture.md)
