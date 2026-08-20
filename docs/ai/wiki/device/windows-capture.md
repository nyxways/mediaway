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
hardware-gated tests pass unchanged. **Resolved 2026-08-20**:
`screen_capture_delivers_zero_copy_frame_or_skip` (`lib_tests.rs`) bounded-polls
until a real frame is delivered and hard-asserts a genuine
`GpuBufferHandle::DirectX11` — real frame delivery through the ring is now
hardware-verified end-to-end (previously blocked on a locked dev-session
desktop). Still **not** a Zero-Copy mark for `CaptureSharing::Shared` (default): the DDA→ring-slot
`CopyResource` below is a real, mandatory GPU→GPU copy in that mode, so Screen stays ✅ (not ⚡)
for it — only fan-out to additional consumers past the first is copy-free. See § Exclusive mode
below for the opt-in path with zero copy at all. See
[ADR-0007](../../../../crates/mediaway-device/adr/windows/0007-ring-buffer-shared-desktop-duplication.md)
for the full design (ring-depth correctness argument, recycle-signal trust
model, ZCA type shape).

Every COM/D3D11 object this module touches is constructed and used only on
the driver thread that owns it — never moved across threads — mirroring the
fix `mediaway-ffi` ADR-0002 landed for `WindowsDeviceHotplug: Send`.
ADR: [0006](../../../../crates/mediaway-device/adr/windows/0006-shared-desktop-duplication.md).

## Exclusive mode — opt-in, zero copy at all (ADR-0008, 2026-08-20)

`DesktopVideoCaptureConfig::sharing: CaptureSharing::Exclusive` (default: `Shared`, above)
bypasses `dxgi_shared` entirely — no driver thread, no ring, no `CopyResource`. `poll_frame`
calls `AcquireNextFrame` directly on the calling thread and hands out the DDA-owned texture
itself; `release_frame` calls `ReleaseFrame`. Opt-in because the caller must know it is (and
stays) the only consumer for this output — a concurrent `open()` (`Shared` or `Exclusive`)
while an `Exclusive` session is alive fails with `CaptureError::AccessDenied`, enforced by DXGI
itself (only one live duplication per output per process), not extra bookkeeping. No live
upgrade to `Shared` — close and reopen instead. Implementation:
`crates/mediaway-device/src/windows_desktop/dxgi_exclusive.rs`.

**Hardware-verified**: `exclusive_screen_capture_delivers_zero_copy_frame_or_skip` (real frame,
real `GpuBufferHandle::DirectX11`, zero copy) and
`exclusive_screen_capture_blocks_second_open_or_skip` (concurrent-open rejection) both pass on
the reference RTX 4090. This revives (narrower, opt-in) the original ADR-0001 design that
ADR-0006's universal registration removed — see
[ADR-0008](../../../../crates/mediaway-device/adr/windows/0008-exclusive-desktop-duplication-zero-copy.md)
for why ADR-0007's earlier rejection of this same idea was revisited.

## Single-shot capture (`capture_video_once` / `capture_next_frame_blocking`)

`mediaway-device` ADR-0006 adds a blocking single-frame primitive
(`VideoCapture::capture_next_frame_blocking`, default-provided, works on any
already-open session) and a facade-level convenience
(`mediaway_device::capture_video_once(open_fn, timeout)`, pays a full
session-open cost every call — do not loop it to build a recorder).
Hardware-verified against the real DXGI backend. See
[capture-once](capture-once.md).
