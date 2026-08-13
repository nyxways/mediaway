# Windows screen capture (DXGI)

- Module: `mediaway-device::windows_desktop`
- API: `WindowsScreenCapture::open` — DXGI Desktop Duplication only
- Output: `Bgra8` + `GpuBufferHandle::DirectX11` (GPU); `release_frame` before next poll
- **Not** window — [windows-window](windows-window.md) · audio — [windows-audio](windows-audio.md)
- Encode: HW MFT may take ARGB32 ([encoder ADR-0005](../../../../crates/mediaway-encoder/adr/windows/0005-bgra-dxgi-input.md))
- ADR: [0001](../../../../crates/mediaway-device/adr/windows/0001-dxgi-desktop-duplication.md)

## Shared, refcounted sessions with ring-buffered fan-out

Every `open()` routes through a shared, refcounted registry keyed by output
identity (`mediaway-device::windows` ADR-0006). A dedicated driver thread
owns the real `IDXGIOutputDuplication` exclusively; per ADR-0007, it
publishes each frame into a fixed-depth (3) ring of GPU textures rather than
copying once per attached consumer. Any number of caught-up consumers share a
published frame via a cheap `Arc` clone — a real Zero-Copy fan-out for the
common case. The one remaining mandatory cost is the DDA→ring-slot
`CopyResource` (required: the DDA-owned resource becomes invalid the instant
`ReleaseFrame` runs). A straggling consumer that falls more than 3 frames
behind degrades to a one-off transient copy for its own lagging frames only —
the driver thread and other consumers are never blocked or affected.
`close()` means "release my interest," not "the OS resource is freed" —
actual `DuplicateOutput` teardown only happens once every attached consumer
has closed. Implementation:
`crates/mediaway-device/src/windows_desktop/dxgi_shared.rs`.

**Verification status:** compiles and passes `cargo clippy --all-targets -- -D
warnings` on real hardware; the pre-existing dual-attach/mismatched-device
hardware-gated tests pass unchanged. Actual frame delivery through the new
ring has **not** been hardware-verified end-to-end — attempted on real
hardware, but the dev session's desktop was locked at the time (DXGI DDA
cannot capture a locked/secure desktop by design). Do not upgrade this page's
or README's Zero-Copy mark until that gap is closed. See
[ADR-0007](../../../../crates/mediaway-device/adr/windows/0007-ring-buffer-shared-desktop-duplication.md)
for the full design (ring-depth correctness argument, recycle-signal trust
model, ZCA type shape).

Every COM/D3D11 object this module touches is constructed and used only on
the driver thread that owns it — never moved across threads — mirroring the
fix `mediaway-ffi` ADR-0002 landed for `WindowsDeviceHotplug: Send`.
ADR: [0006](../../../../crates/mediaway-device/adr/windows/0006-shared-desktop-duplication.md).

## Single-shot capture (`capture_video_once` / `capture_next_frame_blocking`)

`mediaway-device` ADR-0006 adds a blocking single-frame primitive
(`VideoCapture::capture_next_frame_blocking`, default-provided, works on any
already-open session) and a facade-level convenience
(`mediaway_device::capture_video_once(open_fn, timeout)`, pays a full
session-open cost every call — do not loop it to build a recorder).
Hardware-verified against the real DXGI backend. See
[capture-once](capture-once.md).
