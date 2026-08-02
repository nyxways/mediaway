# ADR-0003: Capability / permission probe, separate from opening a session

- **Status**: Accepted
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device`

## Context

Callers currently have no way to ask "would this capture source work" without
attempting a full [`VideoCapture`](../src/video.rs)/[`AudioCapture`](../src/audio.rs)
open — which, for backends without a device, means constructing a config and
inspecting the returned [`CaptureError`](../src/error.rs) variant. There is also
no shared way to ask about OS-level consent (microphone/screen-share
permission) ahead of time.

Each platform crate (`mediaway-device-windows`, `mediaway-device-linux`) has a
different, real answer to "is this granted":

- **Windows**: no consent-dialog API exists for a Win32 desktop app. The only
  way to know if microphone access is denied is to actually open a `WASAPI`
  capture endpoint and see whether it succeeds. Screen/window capture and
  render-loopback audio have no separate consent gate at all.
- **Linux (portal)**: `xdg-desktop-portal`'s `ScreenCast` interface *is* the
  consent mechanism — starting a session is what shows the OS picker/consent
  dialog. There is no cheaper way to ask "is this granted" than doing the real
  handshake once.

A shared vocabulary lets `mediaway-pipeline::platform` dispatch this the same
way it already dispatches `ScreenCapture::open`/`Microphone::open`, without each
call site re-deriving platform-specific meaning from `CaptureError`.

## Decision

> Add `DeviceKind`, `Support`, `Unavailable`, `PermissionState` to the facade
> (`src/capability.rs`). No new trait — each platform crate exposes plain
> `support(kind) -> Support` / `request_permission(kind) -> Result<PermissionState, CaptureError>`
> free functions (mirroring `WindowsScreenCapture::open` et al.), matching
> ADR-0002: the facade holds shared vocabulary only, platform crates hold the
> real logic, and `mediaway-pipeline::platform` is the one place that `cfg`-dispatches
> between them.

- `Support` is a **live** probe, not just "was a backend compiled for this
  OS" — an early draft of this ADR treated it as compile-time-only, but a
  build target answer is not the same as "does this machine actually have
  it right now" (the concrete request that reopened this design: distinguish
  compile-time OS support, real-environment device presence, and OS-version
  gating). `Unavailable` tiers *why* not, as three conditions a caller should
  react to differently: `NotImplemented` (no code — needs a rebuild),
  `OsVersionTooOld` (checked live, e.g. `GraphicsCaptureSession::IsSupported`,
  or a real per-process-loopback activation attempt), `NoDeviceFound` (no
  matching device/service right now, e.g. zero WASAPI endpoints enumerated,
  or no reachable portal — can change without a rebuild).
- `PermissionState` answers a separate, **runtime OS-consent** question — did
  the OS grant it — and is honestly `Unknown` wherever no cheap probe exists
  (Windows screen/window) rather than guessing `Granted`.
- `request_permission` is documented as a **costly path** per
  [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md)
  where it actually opens a real backend session (Windows microphone and
  process-loopback support check; Linux screen) — it is not a free query
  there.
- `DeviceKind::ProcessLoopback` is a separate variant from `Loopback`: render
  loopback has no OS-version gate on Windows, per-process loopback needs
  Windows 10 2004+ — conflating them would hide a real, distinct failure mode.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| A `DeviceCapabilities` trait, implemented per platform | No dynamic-dispatch use case exists (unlike `Box<dyn VideoCapture>`, callers don't hold a capability object across time); a trait with only associated functions and no `&self` state is an abstraction with no caller that needs it. |
| Guess `Granted` when no cheap probe exists | Silently wrong on Windows screen/window capture (secure desktop, ACL-restricted output) — `Unknown` is the honest answer; caller still gets a real answer from `CaptureError::AccessDenied` on actual `open`. |

## Consequences

### Positive

- `mediaway-pipeline` apps can show "camera: not supported yet" instead of
  only discovering it via a failed `open`.
- The one real proactive OS consent trigger this workspace has (Linux portal)
  gets a documented, callable entrypoint.

### Negative / Trade-offs

- Windows `request_permission(Microphone)` has a real cost (opens WASAPI,
  spawns the capture worker thread) — callers must not poll it per frame.
- `PermissionState::Unknown` is an unsatisfying answer for Windows
  screen/window capture; resolving it further would need a real GPU device
  handle or target `HWND` passed into the probe, which this coarse,
  session-free API deliberately does not take.

## References

- [ADR-0002](0002-facade-platform-boundary.md) — facade/platform boundary this follows
- [`docs/ai/wiki/device/capabilities.md`](../../../docs/ai/wiki/device/capabilities.md)
- `mediaway-device-windows/src/capabilities.rs`, `mediaway-device-linux/src/capabilities.rs`
