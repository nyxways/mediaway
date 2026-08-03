# mediaway-device-linux — overview

Linux **platform backend** for capture:

- Screen: `xdg-desktop-portal` `ScreenCast` + PipeWire (CPU copy this session)
- Window: same portal `ScreenCast` session, `SourceType::Window` (CPU copy)
- Camera: V4L2 `mmap` streaming I/O (`YUYV`/`NV12`/`YU12`, CPU copy)
- Microphone: direct PipeWire audio stream, no portal (`F32` PCM)

All four sources are real, complete, and unit-tested; still 👻 in the root codec-support matrix because no portal D-Bus service, PipeWire daemon, or V4L2 device node has been reachable in testing so far.

**Build deps:** `pipewire` crate → `libpipewire-0.3-dev` + `libspa-0.2-dev` (Debian/Ubuntu) at build time; required by CI's Ubuntu job.

| Doc | Role |
|-----|------|
| [roadmap.md](docs/roadmap.md) | Stages |
| [adr/](adr/) | Portal + PipeWire architecture, zero-runtime-verification caveat |
