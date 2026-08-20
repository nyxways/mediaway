# Windows window capture (WGC)

- Module: `mediaway-device::windows_desktop`
- API: `WindowsWindowCapture::open` — WinRT Graphics Capture (`CreateForWindow`)
- Source: `CaptureSource::Window` / `VideoCaptureConfig::window` (≠ Screen)
- Output: `Bgra8` + `DirectX11` Zero-Copy; hold frame until `release_frame`
- **Hardware-verified 2026-08-20** (`wgc_window_capture_delivers_zero_copy_frame_or_skip`,
  `wgc_tests.rs`): real win32 test window + real D3D11 device + real WGC session, bounded poll
  loop, asserts a genuine `GpuBufferHandle::DirectX11` frame — closes `adr/windows/0004`'s own
  "prove capture" acceptance criterion. README Window/Windows cell is now **⚡**.
- Audio pair: `AudioCaptureSource::ProcessLoopback` for same-app sound
- Overlay: `exclude_window_from_capture` so HUD is omitted from DDA/WGC
- Resize: `poll_frame` detects a `Frame.ContentSize` mismatch and calls
  `Direct3D11CaptureFramePool::Recreate` at the new size before delivering the frame
  (previously every frame after a resize was silently skipped forever)
- ADR: [0004](../../../../crates/mediaway-device/adr/windows/0004-wgc-window-capture.md)
