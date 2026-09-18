# mediaway-device — ADRs

| ID | Title | Status |
|----|-------|--------|
| [0001](0001-capture-traits.md) | `VideoCapture` streaming trait | Accepted |
| [0002](0002-facade-platform-boundary.md) | Facade vs `mediaway-device-<platform>` | Accepted |
| [0003](0003-capability-and-permission-probe.md) | Capability / permission probe, separate from opening a session | Accepted |
| [0004](0004-audio-playback-traits.md) | `AudioPlayback` streaming trait | Proposed |
| [0005](0005-device-selection.md) | Device selection: `DeviceId`, `Select`, enumeration, hotplug (audio v1) | Proposed |
| [0006](0006-capture-once-screenshot.md) | Single-shot capture: `capture_next_frame_blocking` + `capture_video_once` | Accepted |
| [0007](0007-gpu-device-factory.md) | GPU device factory — adapter enumeration + configurable DirectX11 device creation | Accepted |
| [0008](0008-cursor-capture-and-wgc-border.md) | Cursor capture as a config field; the WGC border as a Windows option | Accepted |
| [0009](0009-capture-region.md) | Capture a region of the surface, cropped on the GPU or by the pool | Accepted (WGC) |

Template: [`template.md`](template.md)

Crate-local only. Workspace ADRs: [`docs/adr/`](../../../docs/adr/).
