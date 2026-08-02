# Windows screen capture (DXGI)

- Crate: `mediaway-device-windows`
- API: `WindowsScreenCapture::open` — DXGI Desktop Duplication only
- Output: `Bgra8` + `GpuBufferHandle::DirectX11` (GPU); `release_frame` before next poll
- **Not** window — [windows-window](windows-window.md) · audio — [windows-audio](windows-audio.md)
- Encode: HW MFT may take ARGB32 ([encoder ADR-0005](../../../crates/mediaway-encoder-windows/adr/0005-bgra-dxgi-input.md))
- ADR: [0001](../../../crates/mediaway-device-windows/adr/0001-dxgi-desktop-duplication.md)

## Shared, refcounted sessions (not per-session Zero-Copy anymore)

Every `open()` now routes through a shared, refcounted registry keyed by
output identity (`mediaway-device-windows` ADR-0006). A dedicated driver
thread owns the real `IDXGIOutputDuplication` exclusively and fans each frame
out via one `CopyResource` per attached consumer — including the lone-
consumer case. This means `WindowsScreenCapture` is **no longer Zero-Copy**:
it pays a real per-frame GPU copy, in exchange for opening the same output
twice in-process succeeding instead of failing with
`CaptureError::AccessDenied`. `close()` now means "release my interest," not
"the OS resource is freed" — actual `DuplicateOutput` teardown only happens
once every attached consumer has closed. Implementation:
`crates/mediaway-device-windows/src/dxgi_shared.rs`. Hardware-verified: two
concurrent sessions on the same output both receive independent frames.

Every COM/D3D11 object this module touches is constructed and used only on
the driver thread that owns it — never moved across threads — mirroring the
fix `mediaway-device-ffi` ADR-0002 landed for `WindowsDeviceHotplug: Send`.
ADR: [0006](../../../crates/mediaway-device-windows/adr/0006-shared-desktop-duplication.md).

## Single-shot capture (`capture_video_once` / `capture_next_frame_blocking`)

`mediaway-device` ADR-0006 adds a blocking single-frame primitive
(`VideoCapture::capture_next_frame_blocking`, default-provided, works on any
already-open session) and a facade-level convenience
(`mediaway_device::capture_video_once(open_fn, timeout)`, pays a full
session-open cost every call — do not loop it to build a recorder).
Hardware-verified against the real DXGI backend. See
[capture-once](capture-once.md).
