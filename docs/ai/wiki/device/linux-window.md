# Linux window capture: portal `SourceType::Window`

Module: `mediaway-device::linux`, `src/window.rs`. ADR: [`0003-portal-window-capture.md`](../../../../crates/mediaway-device/adr/linux/0003-portal-window-capture.md).

## Shape

`LinuxWindowCapture` is a thin wrapper: `screencast::open_session(SourceType,
media_role, config)` is the shared portal-handshake + `PipeWire`-stream
plumbing behind **both** `LinuxScreenCapture` and `LinuxWindowCapture` — see
[linux-capture.md](linux-capture.md)'s flow diagram, which applies unchanged
here except:

| | Screen | Window |
|-|--------|--------|
| `SourceType` | `Monitor` | `Window` |
| `PipeWire` `MEDIA_ROLE` | `"Screen"` | `"Window"` |
| `CaptureSource` field ignored | `output_index` (portal picker chooses) | `window` handle (portal picker chooses) |

`MEDIA_ROLE` is set via a runtime `Properties::insert` (not the `properties!`
macro, which only accepts compile-time literals) since it varies per caller.

## Key decisions (see ADR-0003 for full rationale)

- `ashpd::desktop::screencast::SourceType::Window` has existed since the
  portal interface's first version — this is a real, small extension of the
  existing screen-capture recipe (ADR-0001), not a new subsystem.
- The `CaptureSource::Window` `window` field (`NativeHandle`) is **ignored** —
  same status as `Screen`'s `output_index`: no portal call exists to target a
  specific window programmatically (unlike `WGC`'s `CreateForWindow(HWND)` on
  Windows). Any value opens the picker.
- Shared internal `Session` type (in `screencast.rs`, `pub(crate)`) carries
  `stream_info`/`poll_frame`/`close` as inherent methods both
  `LinuxScreenCapture` and `LinuxWindowCapture`'s `VideoCapture` impls
  delegate to — avoids duplicating the ~300-line portal/`PipeWire` worker.
- `capabilities::support(DeviceKind::Window)` reuses the same D-Bus
  reachability probe as `Screen` (one portal interface serves both).

## Zero runtime verification (important)

Same gap as screen capture — no portal-capable desktop session in WSL2. See
[linux-capture.md](linux-capture.md) § Zero runtime verification.

## Related

- [linux-capture.md](linux-capture.md) — the screen-capture recipe this shares
- [windows-window.md](windows-window.md) — WGC window capture for comparison
