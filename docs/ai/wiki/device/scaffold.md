# Device crate scaffold

- Path: `crates/mediaway-device` (**facade**); ADRs 0001–0002
- Backends: `mediaway-device::windows` (DXGI screen ⚡ · WGC window 🆗 · WASAPI 🆗) ·
  `mediaway-device::web` · `mediaway-device::linux` (portal + PipeWire screen, CPU
  copy — see [linux-capture](linux-capture.md))
- Screen ≠ Window — separate types (`WindowsScreenCapture` / `WindowsWindowCapture`)
- Mic not ⚡ yet (WASAPI still copies PCM) — [marks](../zero-copy/marks.md)
- Web policy: no programmatic device/window ID — see `mediaway-device::web` ADR-0001
- README Device table: screen ⚡ (Windows) / 🛠️ (Linux, unproven this session) ·
  window 🆗 · mic 🆗
