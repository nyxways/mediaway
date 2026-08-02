# Device selection — `DeviceId` / `Select` / enumeration / hotplug

ADR: [`mediaway-device` ADR-0005](../../../crates/mediaway-device/adr/0005-device-selection.md)
(Accepted — types + Windows `enumerate` + `DeviceLost` wiring + `DeviceHotplug`'s
Windows backend (`WindowsDeviceHotplug`, Microphone/Loopback) are all implemented and
hardware-verified).

## Why

Every capture/playback config selected a device with a raw `device_index: u32` /
`device: usize` / `output_index: u32`, and every Windows backend hard-rejected any non-zero
value — only "the OS default" was reachable, and no enumeration API existed anywhere. Classic
PortAudio integer-index anti-pattern: an index silently means a different physical device
after a hotplug/replug reorders OS enumeration.

## Decision shape

- **`DeviceId`** — one opaque newtype wrapping a private, backend-tagged
  `enum DeviceIdRepr { Wasapi(String), MediaFoundation(String), DxgiOutput(String) }`
  (`#[non_exhaustive]`). One type, not three, mirroring `GpuDeviceHandle`'s "tagged variants
  in one enum" precedent — lets `Select` be reused across every config without generics.
  `Display`/`FromStr` round-trip via a tagged-string prefix.
- **`Select`** — `Default | Id(DeviceId) | NameContains(String)`, all **owned** (not
  borrowed): every backend spawns a `'static` worker thread
  (`thread::spawn(move || ..)`), so a borrowed `Select<'a>` cannot cross that boundary
  without infecting every config struct with a lifetime.
- **`DeviceInfo`** — owned, `Clone + Send + 'static` snapshot: `id`, `kind` (reused
  `DeviceKind` from ADR-0003), `name: String`, `is_default: bool`, `ordinal: u32`. No
  `Name::Restricted` — Windows enumeration/friendly-name queries aren't consent-gated
  for a Win32 desktop app, confirmed from `capabilities.rs::endpoint_support` already
  calling `EnumAudioEndpoints` without consent handling.
- **`DeviceHotplug`** — sync-poll trait (mirrors `AudioCapture::poll_frame`), **audio only
  in v1** via `IMMNotificationClient` (a real, mature WASAPI COM callback).
  `WindowsDeviceHotplug` implements it: a `#[implement(IMMNotificationClient)]` COM server
  object (`src/hotplug.rs`) registered via
  `IMMDeviceEnumerator::RegisterEndpointNotificationCallback`, pushing events into a bounded
  drop-oldest queue that `poll_event` drains non-blockingly — no worker thread, since the OS
  calls the callback directly on its own MTA thread. Camera/screen hotplug deferred — their
  real mechanism (`WM_DEVICECHANGE`/`WM_DISPLAYCHANGE`, message-pump-based) is structurally
  different, needs its own ADR.
- **`CaptureError::DeviceLost` / `PlaybackError::DeviceLost`** — new variant naming a real,
  previously-silent gap: `pump_capture_loop`/`pump_playback_loop` already `break` silently
  on `AUDCLNT_E_DEVICE_INVALIDATED`-shaped failures, leaving a live session looking
  permanently idle instead of erroring.

## Breaking, applied now

`AudioCaptureSource::{Microphone,Loopback}`, `AudioPlaybackConfig`, `CaptureSource::{Camera,
Screen}` all move their index/ordinal field to `select: Select`. `CaptureSource::Window`
**unchanged** (`NativeHandle` — not a persistent device, ADR-0013 already got this right).
`AudioCaptureSource::ProcessLoopback` unchanged (PID-parameterized, never index-based).

## Composes with, doesn't duplicate, ADR-0003

`support(kind)` → cheap "does this kind exist" · `enumerate(kind)` (this ADR) → "list the
actual devices" · `request_permission(kind)` → OS consent, still kind-level (Windows has no
per-device consent granularity to expose). See [capabilities](capabilities.md).

## Status

Types + breaking field changes + `mediaway-device-windows`'s `enumerate` (Microphone/
Loopback/Camera/Screen) + `DeviceLost` wiring + `WindowsDeviceHotplug` are **all done and
hardware-verified**. `Select` resolution: `wasapi.rs::resolve_endpoint` (mic/loopback,
shared with playback), `camera.rs::resolve_camera_index`, `dxgi.rs::resolve_output_index`
(adapter-scoped per the ADR). No `mediaway_pipeline::platform` free function for
`enumerate` — the ADR's own "free-function shape" precedent means callers use
`mediaway_device_windows::enumerate` directly, same as `support`/`request_permission`
today. **Still deferred**: Camera/Screen hotplug (separate mechanism, separate ADR — see
ADR-0005 § Deferred).

## Real-hardware finding: `PKEY_Device_FriendlyName` can collide

Confirmed on a Korean-locale Windows desktop this session (4 capture endpoints, 3
different adapters): `PKEY_Device_FriendlyName` is not always a unique per-endpoint
name — some drivers report only the generic, localized device-*class* label (this
machine's endpoints were fine, but the underlying Windows behavior is well documented
as a real footgun on some driver/locale combinations, not this workspace's invention).
`wasapi.rs::endpoint_friendly_name` now also queries
`PKEY_DeviceInterface_FriendlyName` (the audio adapter/driver's own name) and appends it
in parentheses **only when not already a substring of the endpoint name** — a naive
"always append" version was tried first and produced doubled suffixes
(`"스테레오 믹스 (Realtek(R) Audio) (Realtek(R) Audio)"`) against this exact hardware, since
`PKEY_Device_FriendlyName` here already embeds the driver name. The pure combine logic
(`wasapi.rs::combine_endpoint_and_interface_names`) is unit-tested against both cases.

## References

- [camera-device-handle](camera-device-handle.md) — the Camera micro-decision this supersedes
- [capabilities](capabilities.md) — ADR-0003, the probe axis this composes with
- `mediaway-device-windows/src/camera.rs` — `enumerate_cameras`, `resolve_camera_index`
- `mediaway-device-windows/src/wasapi.rs` — `resolve_endpoint`, `endpoint_friendly_name`
- `mediaway-device-windows/src/hotplug.rs` — `WindowsDeviceHotplug`,
  `NotificationSink` (`IMMNotificationClient` COM server object)
