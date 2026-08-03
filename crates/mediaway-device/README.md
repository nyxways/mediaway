# mediaway-device — docs

Device capture: camera, microphone, screen/window, audio hotplug, capability and
permission probing. Traits and domain facades live here, and the OS backends are
merged in as `#[cfg]`-gated modules per ADR-0021: `camera`, `desktop`, `audio`
(domain facades), `windows`/`windows_camera`/`windows_desktop`/`windows_audio`
(Win32 MF/DXGI/WGC/WASAPI), `linux` (xdg-desktop-portal + PipeWire + V4L2), and
`web` (getUserMedia/getDisplayMedia).

| Doc | Notes |
|-----|-------|
| [roadmap.md](docs/roadmap.md) | Capability stages |
| [adr/](adr/) | Traits + merged backend ADRs (per-module subdirs) |
| [docs/](docs/) | Merged module docs/READMEs |
