# mediaway-device-windows — overview

Windows **platform backend** for capture:

- Screen: DXGI Desktop Duplication (GPU ⚡)
- Window: WGC (`WindowsWindowCapture`, separate from screen)
- Audio: WASAPI mic / loopback / process loopback (🆗 today; CPU ⚡ when shared buffers land)
- Camera: Media Foundation `IMFSourceReader` (hardware-verified with a real USB webcam, not yet wired into the public API)
- Overlay: `exclude_window_from_capture`

| Doc | Role |
|-----|------|
| [roadmap.md](docs/roadmap.md) | Stages |
| [adr/](adr/) | DXGI · WASAPI · exclusion · WGC |
