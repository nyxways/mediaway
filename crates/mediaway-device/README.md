# mediaway-device

<p align="center">
  <a href="https://docs.rs/mediaway-device"><img src="https://img.shields.io/docsrs/mediaway-device" alt="docs.rs"></a>
  <a href="https://crates.io/crates/mediaway-device"><img src="https://img.shields.io/crates/v/mediaway-device.svg" alt="crates.io"></a>
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production; public APIs may
> change without notice. Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack.

Device capture and playback: camera, microphone, screen/window capture, audio
playback, hotplug, and capability/permission probing. Shared vocabulary
(`DeviceKind` / `DeviceId` / `Select`, `Support` / `PermissionState`) plus domain
facades (`camera`, `desktop`, `audio`) over per-OS backends — Windows (DXGI screen,
WGC window, WASAPI mic/loopback/playback), Linux (xdg-desktop-portal + PipeWire + V4L2),
and Web (`getUserMedia` / `getDisplayMedia`).

## Quick start

```rust
use mediaway_common::Rational;
use mediaway_device::desktop::{DesktopVideoCapture, DesktopVideoCaptureConfig};
use mediaway_device::windows_desktop::WindowsScreenCapture; // Windows backend shown
use mediaway_device::Select;

// Screen capture at 30 fps — DXGI Desktop Duplication (Zero-Copy:
// GPU `GpuBufferHandle` frames out).
let mut cap = WindowsScreenCapture::open(&DesktopVideoCaptureConfig::screen(
    Select::Default,
    Rational::new(1, 30),
))?;

loop {
    if let Some(frame) = cap.poll_frame()? {
        // frame.payload is a GpuBufferHandle (⚡) or CPU buffer
        cap.release_frame()?;
    }
}
```

Cross-platform dispatch (per-OS backend selection) lives in
`mediaway::platform::ScreenCapture::open`.

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| Capture/playback traits + capability probing | ✅ | `DeviceKind`, `Support`, `PermissionState` |
| Windows screen capture (DXGI) | ✅ ⚡ | Zero-Copy out |
| Windows window capture (WGC) | ✅ | |
| Windows mic + system/process loopback (WASAPI) | ✅ | |
| Windows audio playback (WASAPI shared mode) | ✅ | |
| Device enumeration (`DeviceId`/`Select`) | ✅ | Hardware-verified |
| Windows camera (Media Foundation) | 🛠️ | Planned |
| Windows hotplug (`IMMNotificationClient`) | 🛠️ | Trait declared; backend pending |
| Web capture (`getUserMedia`/`getDisplayMedia`) | ✅ | Picker only; no programmatic selection |
| Linux capture (portal + PipeWire + V4L2) | 👻 | Implemented; hardware verification pending |
| Single-shot capture (`capture_video_once`) | 🛠️ | Design done (ADR-0006), not implemented |

## Docs

- [`docs/roadmap.md`](docs/roadmap.md) — stages, deferred items, design decisions
- [`mediaway`](../mediaway/) — convenience pipeline (`platform::ScreenCapture` etc.)
- Root [README](../../README.md) — device support matrix

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
