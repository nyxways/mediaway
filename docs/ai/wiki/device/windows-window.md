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
- Cursor: `config.cursor` → `SetIsCursorCaptureEnabled`; failure is an **error in both
  directions** (WGC's own default *includes* the pointer, so a failed `Excluded` would leak it)
- Yellow border: `open_with(config, options)` with `options.border = CaptureBorder::Hidden` →
  `RequestAccessAsync(Borderless)` + `SetIsBorderRequired(false)`, **read back** into
  `border_hidden()`. A refusal does not fail the open (the border is on screen, never in
  frames). Unpackaged process on Win11 26100: granted with no prompt. Until 2026-09-18 a
  comment claimed the border was hidden, but nothing ever called `SetIsBorderRequired`.
- Even crop: `options.dimensions = FrameDimensions::EvenCropped` sizes the frame pool to
  the content rounded down to even — free, because **WGC crops to the pool, never scales**
  (measured: 0 of 3.3 M pixels differ). `ContentSize` still reports the full window, so
  resize detection compares against the *target* pool size. Frame size comes from the
  texture (`GetDesc`), since the frame in hand after a resize is from the old pool.
- ADR: [0004](../../../../crates/mediaway-device/adr/windows/0004-wgc-window-capture.md) ·
  cursor/border [0008](../../../../crates/mediaway-device/adr/0008-cursor-capture-and-wgc-border.md)
