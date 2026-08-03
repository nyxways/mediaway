# ADR-0004: Per-domain Cargo feature split (camera / desktop / audio / hotplug)

- **Status**: Accepted
- **Date**: 2026-08-01
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device-ffi`

## Context

This crate's Rust dependencies (`mediaway-device-camera`/`-desktop`/`-audio`,
`mediaway-device-windows-camera`/`-desktop`/`-audio`) were split by domain — see
`mediaway-device/adr/0007-domain-crate-split.md`. Before this ADR, this crate's own C
ABI surface did not follow that split: `video.rs` mixed Camera (Media Foundation) and
Screen/Window (DXGI/WGC) behind one `video` Cargo feature, and `audio.rs` mixed
Microphone (a real input device) with Loopback/`ProcessLoopback` (desktop-render
capture) behind one `audio` feature. A consumer who only wanted, say, microphone
capture still linked the Camera and DXGI/WGC backend code into their native library,
because the feature granularity didn't match the domain boundaries.

A downstream consumer (the `Mediaway.Device.*` C# NuGet package split, deferred
pending this ADR) explicitly wants a caller who only needs one capability — e.g. "just
the microphone" — to not link unrelated backend code (Camera's Media Foundation
plumbing, DXGI/WGC's Desktop Duplication/Graphics Capture plumbing) into their native
asset at all. Cargo's conditional compilation is the only mechanism in this crate's
toolchain that removes code from a build, not just from a public entry-point list — a
shared single native library with narrower per-consumer P/Invoke declarations does not
achieve this, since the unused code stays compiled into that one shared binary
regardless of which functions a given consumer calls.

## Decision

> Replace the two Cargo features `video`/`audio` with four: `camera`, `desktop`,
> `audio`, `hotplug` — each maps to its own C module and its own Windows backend crate
> dependency, so disabling a feature genuinely removes that domain's code from the
> build.

- `camera` → `camera.rs` (Camera only, CPU-only, no `gpu_device` field on its config —
  see `mediaway-device-camera`). Depends on `mediaway-device-camera` +
  `mediaway-device-windows-camera` directly, **not** the `mediaway-device-windows`
  orchestrator crate.
- `desktop` → `desktop_video.rs` (Screen/Window, GPU-capable) **and**
  `desktop_audio.rs` (Loopback/`ProcessLoopback`) — grouped under one feature, matching
  the Rust facade grouping: both capture what the desktop is already doing, not a real
  input device. Depends on `mediaway-device-desktop` +
  `mediaway-device-windows-desktop` directly.
- `audio` → `audio.rs` (Microphone only). Depends on `mediaway-device-audio` +
  `mediaway-device-windows-audio` directly.
- `hotplug` → `hotplug.rs` (unchanged internally). **Exception to the "no orchestrator"
  rule**: v1 hotplug scope spans Audio I/O (`DeviceKind::Microphone`) and Desktop
  (`DeviceKind::Loopback`) kinds, and its real backend (`WindowsDeviceHotplug`) lives in
  the `mediaway-device-windows` orchestrator crate for that reason (see that crate's own
  module doc — moving it out would contradict its documented "spans both domains"
  rationale). Enabling `hotplug` on Windows therefore still transitively links all three
  domain backends. This is a deliberate, disclosed trade-off, not an oversight — see
  § Consequences.

Camera/Screen video frame and pixel-format types are duplicated per domain rather than
shared: `MediawayCameraFrame` (CPU-only, no `storage_kind`/`gpu_buffer` fields) vs.
`MediawayDesktopFrame` (GPU-capable). A shared frame struct would force
`MediawayCameraFrame` to carry GPU fields it can never populate, or force the GPU
handle types (`MediawayGpuDeviceHandle`/`MediawayGpuBufferHandle`) to be compiled even
when `desktop` is disabled. Audio types split the same way:
`MediawayAudioCaptureConfig`/`MediawayDeviceAudioFrame` (Microphone, `audio` feature)
vs. `MediawayDesktopAudioCaptureConfig`/`MediawayDesktopAudioFrame` (Loopback/
`ProcessLoopback`, `desktop` feature).

Status codes (`MediawayDeviceStatus` in `status.rs`) and the Rational timebase type
stay ungated — cheap, feature-independent, and gating them would make the status
enum's numeric layout vary by which features are enabled, a real ABI hazard for a
consumer linking a subset build against a header generated from a full build.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Keep one shared native library; narrow only the C# P/Invoke surface per NuGet package | Does not remove unused backend code from the shipped binary — only hides it from one consumer's bindings. Explicitly rejected: the goal is code exclusion, not API surface tidiness. |
| Split into physical crates (`mediaway-camera-ffi`/`mediaway-desktop-ffi`/`mediaway-audio-ffi`), mirroring the Rust facade/backend split exactly | Real multi-crate split changes the shipped artifact shape (separate `cdylib`s, separate headers, separate ABI version numbers) for a crate that has not shipped a header yet (`publish = false`) — strictly more churn than a feature split for the same code-exclusion outcome. Also complicates `hotplug.rs`'s existing ADR-0002 bridging-thread design, which is cross-domain by nature. Revisit if/when this crate ships and a genuinely independent release cadence per domain is needed. |
| Move `WindowsDeviceHotplug` out of the orchestrator into `mediaway-device-windows-audio` so `hotplug` never needs Camera/Desktop | Contradicts that type's own documented rationale for living in the orchestrator (watches kinds spanning Audio I/O *and* Desktop, not Audio alone) — v1 scope already includes `DeviceKind::Loopback`, a Desktop-owned kind. Rejected without a corresponding decision to also demote Loopback out of hotplug's v1 scope, which is out of this ADR. |

## Consequences

### Positive

- A caller who only needs Microphone capture builds a release DLL with the
  Camera/DXGI/WGC backend code physically absent — measured empirically:
  `--no-default-features --features audio` release build is 242 KB vs. 480 KB for all
  four features enabled (same measurement methodology already used for the pre-ADR
  `audio`/`video` split).
- Feature boundaries now match the Rust facade/backend crate boundaries 1:1 (except
  `hotplug`), keeping the two splits mentally consistent.
- Frame/config types no longer carry domain-irrelevant dead fields (Camera's config had
  a permanently-`NONE` `gpu_device` before this ADR).

### Negative / Trade-offs

- `hotplug`-only builds on Windows still transitively link Camera/Desktop/Audio backend
  code via the orchestrator crate — not zero-cost. Accepted because hotplug's own
  scope is genuinely cross-domain (§ Decision), and the orchestrator's own three
  backend crates are comparatively small next to their own domain's driver
  surface (Media Foundation/DXGI/WGC device access), unlike, say, an encoder crate.
- More C symbols and header sections than the pre-ADR two-feature shape (Camera and
  Screen/Window no longer share `mediaway_video_capture_config_t`/`_open`/etc.) —
  intentional; a shared config struct is exactly what made independent exclusion
  impossible.
- Breaking C ABI rename (`mediaway_video_capture_*` → `mediaway_camera_capture_*` /
  `mediaway_desktop_capture_*`; `mediaway_audio_capture_config_loopback`/
  `_process_loopback` moved to `mediaway_desktop_audio_capture_config_*`). Acceptable
  only because this crate has never shipped a header (`publish = false`,
  `include/mediaway/device.h` is regenerated by hand, not yet distributed) — the same
  reasoning ADR-0003 relied on for its own breaking change.

## References

- `mediaway-device/adr/0007-domain-crate-split.md` — the Rust facade/backend split
  this ADR mirrors at the C ABI layer.
- `adr/0001-capture-c-abi.md`, `adr/0002-callback-event-delivery.md`,
  `adr/0003-gpu-handle-c-abi.md` — prior C ABI design decisions this ADR amends the
  module/feature shape of, without changing their panic-safety/ownership/hotplug
  designs.
- `docs/spec/c-ffi.md` — per-capability `*-ffi` crates and minimal-default-features
  policy this ADR applies at Cargo-feature granularity within one crate.
