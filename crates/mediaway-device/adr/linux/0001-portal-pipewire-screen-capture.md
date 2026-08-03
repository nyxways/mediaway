# ADR-0001: `xdg-desktop-portal` ScreenCast + PipeWire for Linux screen capture

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device-linux`

## Context

Linux screen capture needs a path that works on the **Wayland** compositors now
default on major distros (GNOME/Mutter, KDE/KWin, wlroots-based). Wayland's
security model has no cross-application `XGetImage` / `XShmGetImage` /
`XCompositeNameWindowPixmap` equivalent — those X11 APIs (legacy, still the
common Linux screen-capture recipe in older tooling) simply do not work under a
Wayland session; a client cannot read another client's (or the
compositor's) buffers directly.

The portal-mediated recipe every major desktop now ships is:

1. `org.freedesktop.portal.ScreenCast` (D-Bus, via `xdg-desktop-portal` +
   a desktop-specific backend: `xdg-desktop-portal-gnome`,
   `-kde`, `-wlr`, …) — session + source-picker + permission prompt.
2. The portal hands back a **PipeWire** node id + a PipeWire "remote" file
   descriptor; the actual video (and, later, audio) frames flow over a
   PipeWire stream, not over D-Bus.

This is the closest Linux analog to Windows' WGC/DXGI: portal-mediated,
permission-prompted, compositor-owned buffers — not a raw, unmediated OS
buffer grab.

## Decision

> For [`CaptureSource::Screen`](../../mediaway-device/src/video.rs) (`output_index` must be
> `0` — see below) with [`CaptureOutputPreference::CpuFramesOk`](../../mediaway-device/src/video.rs):
>
> 1. `ashpd::desktop::screencast::Screencast` — `create_session` → `select_sources`
>    (`SourceType::Monitor`, `CursorMode::Hidden`, `multiple = false`,
>    `PersistMode::DoNot`) → `start` → `open_pipe_wire_remote`.
> 2. Connect via the `pipewire` crate: `Context::connect_fd` on the portal's
>    remote fd, build a `Video`/`Capture`/`Screen` stream targeting the
>    negotiated node id, `StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS`.
> 3. `SPA_PARAM_EnumFormat` offers only `BGRx` / `RGBx` / `RGBA` / `I420` — the
>    formats [`format::map_spa_video_format`](../src/format.rs) can map to
>    [`PixelFormat`](../../mediaway-common/src/formats.rs). `MAP_BUFFERS` means the
>    server negotiates a **mappable** SPA buffer type (`MemPtr`/`MemFd`), not
>    `DmaBuf` — see § Zero-Copy status below.
> 4. `poll_frame` copies the mapped buffer's chunk bytes into an owned
>    `VideoFrameStorage::Cpu`. `release_frame` is a no-op (no GPU handle is ever
>    held).
>
> `output_index != 0` → `CaptureError::Unsupported`: the portal's own picker UI
> selects the monitor/window interactively; there is no
> `org.freedesktop.portal.ScreenCast` call to pick output *N* programmatically
> the way `IDXGIAdapter::EnumOutputs(index)` does on Windows.
>
> `CaptureOutputPreference::ZeroCopyGpu` → `CaptureError::Unsupported`
> (see § Zero-Copy status — never silently hand back CPU frames when the
> caller asked for GPU Zero-Copy).
>
> Window capture (`CaptureSource::Window`) and audio capture (`AudioCapture`,
> PipeWire audio streams) are **explicitly deferred** — not implemented this
> session. See `docs/roadmap.md`.

### Zero-Copy status: CPU copy this session, DMA-BUF deferred

PipeWire can negotiate a `DmaBuf`-backed SPA buffer (a GPU-resident dma-buf fd
+ DRM format modifier) instead of a mappable one, which would be the genuine
Zero-Copy analog to DXGI's `ID3D11Texture2D`. This backend **does not**
negotiate or accept `DmaBuf` buffers this session:

- A dma-buf fd is not, by itself, a `mediaway_common::GpuBufferHandle` — the
  existing variants (`DirectX11/12`, `Vulkan{image, memory}`, `Metal`,
  `AndroidSurface`, `WebGpu`) all name an **already-imported** GPU API object.
  Turning a raw dma-buf fd into one (e.g. importing into a `VkImage` via
  `VK_EXT_external_memory_dma_buf` + `VK_EXT_image_drm_format_modifier`, or an
  `EGLImage`) is a real GPU-API-specific step that belongs in whichever
  encoder/consumer backend picks a GPU API — not something this capture crate
  should decide unilaterally by adding a new `GpuBufferHandle::DmaBuf { fd, .. }`
  variant to the **shared** `mediaway-common` crate without a dedicated,
  reviewed workspace decision.
- Doing the import ourselves would mean a **third** new dependency this
  session (`ash`/`vulkano`, or EGL bindings) on top of `ashpd` + `pipewire`,
  which the dependency review below already flags as needing extra rigor.
  Landing that with **zero ability to run or observe the result** (see below)
  is not a responsible trade.
- `MAP_BUFFERS` + the `EnumFormat` choice list above requests exactly the
  mappable (`MemPtr`/`MemFd`) case, so `poll_frame` never has to guess: if the
  compositor still returns a `DmaBuf`-typed `Data` (`data.type_() ==
  DataType::DmaBuf`), the frame is dropped rather than silently mis-read as
  mapped memory.

Consequence: `VideoFrameStorage::Cpu` only, this session, and
`CaptureOutputPreference::ZeroCopyGpu` is rejected rather than silently served
from CPU (ADR-0006 — never present a copy path as Zero-Copy, never silently
choose the slow path). Landing the GPU dma-buf import is a **follow-up ADR**
once (a) a maintainer has decided which `GpuBufferHandle` shape to add in
`mediaway-common`, and (b) real hardware exists to verify it.

### `BGRx` / `RGBx` → `Bgra8` / `Rgba8` approximation

`BGRx` and `RGBx` carry an unused 4th byte, not real alpha. Mapping them to
`PixelFormat::Bgra8` / `Rgba8` is the same approximation the Windows DXGI
backend already makes for its BGRA desktop surface (`mediaway-device-windows`
ADR-0001) — documented in `format::map_spa_video_format` rustdoc; consumers
must not read that byte as meaningful alpha.

## Dependency review (`docs/conventions/deps-policy.md`)

Two new crates — more scrutiny than a single-dependency addition.

### `ashpd` 0.13

| Check | Answer |
|-------|--------|
| Need | No `std`/existing-workspace-dep/~50-line alternative: the `ScreenCast` portal is a D-Bus interface with session lifecycle, request/response signals, and file-descriptor passing (`open_pipe_wire_remote`) — hand-rolling a `zbus` client for this is a large, easy-to-get-subtly-wrong surface (session handles, request object paths, signal matching). |
| License | MIT — allowed (`deny.toml`). |
| Transitive | `zbus` (MIT/Apache-2.0), `zvariant`, `enumflags2`, `serde` family — all permissive; no GPL/FFmpeg surprise found. |
| Maintenance | Actively maintained (`bilelmoussaoui/ashpd`), ~11.6M crates.io downloads, 269 dependent crates (46 direct) — this **is** the de-facto Rust xdg-desktop-portal client, used by GNOME/Flatpak-adjacent tooling. |
| Features | `default-features = false` + `["screencast", "async-io"]` only. Default feature is `tokio` — explicitly **not** taken (no other workspace crate depends on tokio yet; `async-io` keeps the executor to the smol-rs reactor already implied by `zbus`'s async design). ashpd's own `pipewire` feature is deliberately **not** enabled — it pins `pipewire = "^0.9"` (`pipewire-sys` 0.9.2), which conflicts with the `pipewire = "0.10"` this crate depends on directly (§ `pipewire` row below): both declare the same Cargo `links = "pipewire-0.3"` key, and cargo rejects two crates claiming one `links` key in the same graph (confirmed by a real `cargo check` failure in WSL2 before this was fixed). We don't need that feature — it only wires ashpd's own `From<pipewire::Error>` conversion, unused since `portal::map_ashpd_error` maps `ashpd::Error` directly. |
| Unsafe/FFI | None directly; `zbus` talks D-Bus over a Unix socket in safe Rust. |

### `pipewire` 0.10

| Check | Answer |
|-------|--------|
| Need | The actual capture bytes/DMA-BUF come over a PipeWire stream (`libpipewire`) once the portal hands back a node id + fd — there is no pure-Rust or std alternative; this is a binding to the system media server, unavoidable for the PipeWire-backed portal recipe. |
| License | MIT — allowed. |
| Transitive | `pipewire-sys` (MIT) — FFI bindings, links system `libpipewire-0.3` via `pkg-config` at build time (**system library, not vendored/GPL** — same shape as `windows-rs` linking system DLLs). |
| Maintenance | Official binding, `gitlab.freedesktop.org/pipewire/pipewire-rs`, maintained by the PipeWire project itself; ~1.2M downloads. `docs.rs` fails to build it (no libpipewire headers in that sandboxed build env) — expected for a system-linked crate, not a maintenance red flag; consistent with needing `libpipewire-0.3-dev` + `pkg-config` locally (see § Environment). |
| Features | Default features taken (no bloated default feature set like `windows-rs`; version-gate features are additive, not disableable without knowing the minimum libpipewire on target — left as-is). |
| Unsafe/FFI | The Rust API surface used here (`MainLoopBox`, `ContextBox`, `StreamBox`, `spa::pod` builders) is documented as "a safe API" over `libpipewire`; `unsafe` (if any) is confined inside `pipewire`/`pipewire-sys`, not this crate. See crate root lint attribute. |

### Alternatives considered and rejected

| Alternative | Why not |
|-------------|---------|
| Raw X11 (`XShm`/`XComposite` via `x11rb` or FFI) | Does not work for cross-application capture under Wayland, now the default compositor protocol on GNOME/Fedora/current Ubuntu; would only cover a shrinking Xorg-session subset. Rejected per `docs/roadmap.md` platform intent (capture the mainstream case). |
| Hand-rolled `zbus` portal client (no `ashpd`) | Reinventing `ashpd`'s session/request/signal plumbing for one portal interface, with zero ability to test it here — strictly higher risk than depending on the crate every other Rust screen-capture project in this space already uses. |
| GStreamer (`pipewiresrc` via `gstreamer-rs`) | Pulls in the entire GStreamer runtime/plugin graph as a dependency merely to reach PipeWire — far heavier than the direct `pipewire` binding for a single stream; GStreamer's own license mix also needs separate review this ADR does not scope. |
| Wayland protocol capture (`wlr-screencopy`, compositor-specific) | Compositor-specific (wlroots only), not portal-mediated/permission-prompted, and does not cover GNOME/KDE — narrower than the portal recipe this ADR targets. |

## Zero runtime hardware/session verification this session

**No real desktop portal session, no real PipeWire stream, and no real
ALSA/PipeWire audio device were exercised in this development session.** The
environment is a WSL2 Ubuntu 24.04 instance with no physical monitor/compositor
and no portal-capable desktop session (`xdg-desktop-portal` + a desktop backend
require a running session bus **and** a compositor implementing the backend
D-Bus interface — WSLg does not provide either). Verification this session was
**compile-only** (`cargo check` / `cargo clippy` for `mediaway-device-linux`
under WSL2, plus a full-workspace native-Windows `cargo check`) — never a
successful `LinuxScreenCapture::open` call, never a received frame, never a
confirmed format negotiation. The hardware/session-gated unit test
(`screencast_tests.rs`) is written to run the real path and is expected to
**skip** here for exactly this reason; it is not a substitute for real
verification on a portal-capable Linux desktop. Unlike
`mediaway-device-windows` (DXGI/WGC/WASAPI all had real hardware verification
in earlier sessions), this crate has **none** yet.

## Consequences

### Positive

- Matches how every current Wayland desktop actually exposes screen capture to
  sandboxed/unprivileged clients — works on GNOME, KDE, and wlroots-based
  compositors that ship a portal backend.
- Honest about the CPU-copy nature of this session's output — no false
  Zero-Copy claim.

### Negative / Trade-offs

- Two new dependencies, one of them (`pipewire`) requiring a system library +
  `pkg-config` at build time — this crate cannot even `cargo check` on a
  machine missing `libpipewire-0.3-dev`.
- Requires a portal + compositor + PipeWire session at runtime; no fallback
  for headless/CI Linux without a compositor (out of scope here — see
  `docs/conventions/testing.md` `_or_skip` pattern, same shape as Windows'
  hardware-gated tests).
- CPU copy only; GPU Zero-Copy via DMA-BUF is real future work, not delivered
  here.
- `output_index` is effectively decorative (must be `0`) — the portal picker,
  not this crate, chooses the monitor; callers porting Windows multi-monitor
  logic 1:1 will need to special-case Linux.

## References

- [`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md)
- [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md)
- [`docs/conventions/deps-policy.md`](../../../docs/conventions/deps-policy.md)
- Windows precedent: [`mediaway-device-windows/adr/0001-dxgi-desktop-duplication.md`](../../mediaway-device-windows/adr/0001-dxgi-desktop-duplication.md)
- Facade boundary: [`mediaway-device/adr/0002-facade-platform-boundary.md`](../../mediaway-device/adr/0002-facade-platform-boundary.md)
- `ashpd`: <https://github.com/bilelmoussaoui/ashpd> (MIT)
- `pipewire`/`pipewire-sys`: <https://gitlab.freedesktop.org/pipewire/pipewire-rs> (MIT)
- PipeWire DMA-BUF sharing: <https://docs.pipewire.org/page_dma_buf.html>
