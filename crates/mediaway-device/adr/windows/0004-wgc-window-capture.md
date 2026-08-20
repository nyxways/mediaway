# ADR-0004: WGC window capture (separate from screen)

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device-windows`

## Context

App / window recording must not be a crop of DXGI Desktop Duplication. WinRT **Graphics Capture** (`CreateForWindow`) targets one `HWND` and yields DX11 textures. Screen stays on DXGI DDA ([ADR-0001](0001-dxgi-desktop-duplication.md)).

## Decision

> Expose [`WindowsWindowCapture`](../src/wgc.rs) for [`CaptureSource::Window`](../../mediaway-device/src/video.rs):
>
> 1. Require caller `d3d11_device` + non-zero `HWND` bits.
> 2. `IGraphicsCaptureItemInterop::CreateForWindow` → free-threaded frame pool → `TryGetNextFrame`.
> 3. Output `Bgra8` + `GpuBufferHandle::DirectX11`; hold the WGC frame until `release_frame`.
> 4. Do **not** fold window into `WindowsScreenCapture`.
>
> Companion audio for one process: [`AudioCaptureSource::ProcessLoopback`](../../mediaway-device/src/audio.rs).

## Consequences

- README Window cell can move toward 🆗/⚡ once CI machines prove capture. **Resolved
  2026-08-20**: `wgc_tests.rs::hardware::wgc_window_capture_delivers_zero_copy_frame_or_skip`
  creates a real win32 window, a real D3D11 device, and a real WGC session, then bounded-polls
  until a real `GpuBufferHandle::DirectX11` frame is delivered — hardware-verified on the
  reference RTX 4090. README Window/Windows cell is now **⚡**.
- Content-size changes: [`WindowsWindowCapture::poll_frame`](../src/wgc.rs) now detects a
  `Frame.ContentSize` mismatch against the pool's last-known geometry and calls
  `Direct3D11CaptureFramePool::Recreate` at the new size before delivering the frame
  (using the frame's own `ContentSize`, not the stale pre-resize geometry) — the standard
  pattern from Microsoft's WGC samples. Stage 1 used to silently skip every frame after a
  resize forever (the stale-geometry comparison never matched again); this closes that gap.
  The size-comparison decision (`resized_geometry`) is a pure function unit-tested in
  [`wgc_tests.rs`](../src/wgc_tests.rs); driving an actual hardware resize end-to-end is not
  automated in this suite (see that file for why).

## References

- live-recorder WGC (reference only) · MS `IGraphicsCaptureItemInterop`
