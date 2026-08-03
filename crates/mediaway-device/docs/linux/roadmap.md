# mediaway-device-linux — roadmap

Linux `xdg-desktop-portal` `ScreenCast` + `PipeWire` + `V4L2` capture backend.
Facade: [`mediaway-device`](../../mediaway-device/docs/roadmap.md).
Platform order: Windows → Web → **Linux** → other. Workspace index:
[`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 0 — Scaffold

- [x] Workspace member + docs / ADR surface
- [x] Stub on non-Linux hosts → `Unsupported`

### 1 — Portal + PipeWire screen capture (CPU copy)

- [x] `CaptureSource::Screen` (`output_index == 0` only) + `LinuxScreenCapture`
- [x] `ashpd` `ScreenCast` session handshake → PipeWire node id + remote fd
- [x] `pipewire` stream connect, `EnumFormat` limited to `BGRx`/`RGBx`/`RGBA`/`I420`
- [x] `poll_frame` → `VideoFrameStorage::Cpu` (copy); `release_frame` no-op
- [x] `CaptureOutputPreference::ZeroCopyGpu` → `Unsupported` (honest — no GPU path yet)
- [ ] **Zero runtime verification this session** — no portal-capable desktop
      session was available (WSL2, no compositor). Compile-checked only. First
      real run on a Wayland/portal-capable Linux desktop still owed.
- [ ] DMA-BUF Zero-Copy import (needs a `mediaway-common::GpuBufferHandle`
      decision + a GPU-API dependency — deferred, see ADR-0001)

### 2 — Camera capture (V4L2, CPU copy)

- [x] `CaptureSource::Camera` + `LinuxCameraCapture` (see ADR-0002)
- [x] `/dev/video*` enumeration filtered to `V4L2_CAP_VIDEO_CAPTURE` nodes
- [x] Format negotiation: `YUYV` > `NV12` > `YU12` (I420); rejects driver
      substitution of a different `FourCC` than requested
- [x] `mmap` streaming I/O (`v4l` crate), stride-aware CPU copy into
      `VideoFrameStorage::Cpu`
- [x] `capabilities::support`/`request_permission(Camera)` — real node
      enumeration / real open-and-close probe
- [ ] `MJPG`/compressed-format support (needs a JPEG decoder dependency —
      out of scope this session)
- [ ] **Zero runtime verification this session** — WSL2 has zero
      `/dev/video*` nodes. Compile-checked only.
- [ ] `CaptureOutputPreference::ZeroCopyGpu` (DMA-BUF export via
      `VIDIOC_EXPBUF` + GPU import) — deferred, same shape as stage 1's GPU gap

### 3 — Window capture (portal `SourceType::Window`)

- [x] `CaptureSource::Window` + `LinuxWindowCapture` (see ADR-0003) — shares
      `screencast.rs`'s portal + PipeWire plumbing via `open_session`
- [x] `capabilities::support`/`request_permission(Window)` — same portal
      probe as `Screen` (shared D-Bus interface)
- [ ] **Zero runtime verification this session** — same portal/compositor
      gap as stage 1

### 4 — Microphone capture (direct PipeWire audio stream)

- [x] `AudioCaptureSource::Microphone { device_index: 0 }` +
      `LinuxMicrophoneCapture` (see ADR-0004) — no portal, connects directly
      to the local PipeWire daemon socket
- [x] `F32LE` interleaved PCM only (same restriction as
      `mediaway-device-windows` WASAPI); bounded drop-oldest queue
      (`PCM_QUEUE_CAP = 64`)
- [x] `capabilities::support(Microphone)` — real (cheap, non-streaming)
      PipeWire daemon connect probe; `request_permission` reports `Granted`
      once reachable (no consent gate exists for this operation on
      PipeWire-based desktop Linux)
- [ ] Non-default source targeting (`PW_KEY_TARGET_OBJECT`) — deferred
- [ ] `Loopback`/`ProcessLoopback` (monitor-port capture) — deferred, not
      attempted this session
- [ ] **Zero runtime verification this session** — WSL2 has no running
      PipeWire daemon (only dev headers). Compile-checked only.

### 5 — Capability / permission probe

- [x] `capabilities::support(Screen)` — real D-Bus round trip confirming the
      portal `ScreenCast` interface is reachable, no session created
- [x] `capabilities::request_permission(Screen)` — real portal handshake
      (`create_session`/`select_sources`/`start`), the same call that shows
      the desktop's screen-share consent dialog; closed immediately after
- [x] `Window`/`Camera`/`Microphone` — real probes, see stages 2–4 above
- [ ] `Loopback`/`ProcessLoopback` — still `Unavailable::NotImplemented` /
      `PermissionState::NotSupported` (no backend)
- [ ] **Zero runtime verification this session** (same caveat as stages 1–4)
      — all `_or_skip` tests compile-check and run in WSL2 but report no
      portal/device/daemon, never observed against real hardware/sessions
