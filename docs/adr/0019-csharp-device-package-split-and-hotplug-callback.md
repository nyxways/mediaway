# ADR-0019: C# `Mediaway.Device.*` package split + real native hotplug callback

- **Status**: Accepted — implemented 2026-08-01
- **Date**: 2026-08-01
- **Deciders**: @dev-nyxie (+ agent)

## Context

ADR-0017 shipped one `Mediaway.Device` package wrapping the (then-unified)
`mediaway-device-ffi` C ABI: Camera, Microphone, and `DeviceHotplug` (poll
mode only — push-mode was explicitly deferred, ADR-0017 § Deferred). Two
things changed since:

1. `mediaway-device-ffi` itself split into four Cargo features — `camera`,
   `desktop`, `audio`, `hotplug` — each mapping to its own C symbol set and,
   on Windows, its own backend crate dependency
   (`mediaway-device-ffi/adr/0004-domain-feature-split.md`), so that a
   consumer who only wants (say) the microphone no longer links
   Camera/DXGI/WGC driver code into their native asset. The old
   `mediaway_video_capture_*`/unified `mediaway_audio_capture_*` C symbols
   this package's `NativeMethods.cs` declared no longer exist.
2. The user asked for the deferred push-mode callback
   (`mediaway_device_hotplug_register_callback`) to be wired for real,
   "for performance" — no more C#-managed polling thread standing in for it.

## Decision

> Split `Mediaway.Device` into a thin shared base plus four leaf packages —
> `Mediaway.Device.Camera`, `Mediaway.Device.Desktop`, `Mediaway.Device.Audio`,
> `Mediaway.Device.Hotplug` — mirroring the Rust FFI split 1:1. Wire
> `DeviceHotplug`'s push mode onto the real native callback via
> `GCHandle` + `UnmanagedCallersOnly` (net8.0) / a `static readonly` delegate
> (netstandard2.0), not a C#-managed polling thread.

### Package layout

- **`Mediaway.Device`** (base, slimmed) — `DeviceKind`, `MediawayDeviceStatus`,
  `MediawayDeviceException`, `CaptureUnavailableException`. No P/Invoke of its
  own. `[InternalsVisibleTo]` grants each leaf package access to
  `MediawayDeviceException.ThrowIfError` — one status→exception mapping,
  not duplicated four times.
- **`Mediaway.Device.Camera`** — `Camera`, `IVideoCapture`, `VideoFrame`
  (CPU-only — no `GpuDevice`/`GpuBuffer` fields at all, matching
  `MediawayCameraCaptureConfig`'s own drop of that dead field). Ported from
  the old package onto `mediaway_camera_capture_*`.
- **`Mediaway.Device.Audio`** — `Microphone`, `IAudioCapture`, `AudioFrame`
  (Microphone only). Ported onto `mediaway_audio_capture_*`.
- **`Mediaway.Device.Desktop`** (new — Screen/Window video was never shipped
  in C# before this) — `DesktopScreenCapture` (`IDesktopVideoCapture`,
  `DesktopVideoFrame`, `GpuDeviceHandle`/`GpuBufferHandle`) and
  `DesktopAudioCapture` (Loopback/ProcessLoopback, `IDesktopAudioCapture`,
  `DesktopAudioFrame`) — grouped together per
  `mediaway-device-ffi/adr/0004-domain-feature-split.md`'s own grouping
  (both capture what the desktop is already doing, not a real input
  device). See § Desktop specifics below.
- **`Mediaway.Device.Hotplug`** — `DeviceHotplug`, now poll **and** real
  push mode. See § Native callback below.

**No type sharing across leaf packages** beyond what already lived in
`Mediaway.Common` (`PixelFormat`, `Rational`) — `SampleFormat` moves there
too (numerically identical across every native header, same reasoning
`PixelFormat`'s own doc already gives). `IAudioCapture`/`AudioFrame` are
**not** shared between `Mediaway.Device.Audio` and `Mediaway.Device.Desktop`
— `Mediaway.Device.Desktop` defines its own `IDesktopAudioCapture`/
`DesktopAudioFrame`, deliberately named to avoid an ambiguous-reference
error for a consumer referencing both packages together (mic + desktop
loopback in one app is a realistic combo). Mirrors this workspace's
existing `Mediaway.Pipeline.VideoFrame` vs. `Mediaway.Device.Camera.VideoFrame`
precedent (different ownership semantics, never unified) and the Rust FFI
layer's own per-domain struct duplication.

### A real naming footgun found and fixed

`namespace Mediaway.Device.Camera { public static class Camera { ... } }` —
a namespace segment and a type sharing the simple name `Camera` — breaks for
any consumer whose **own** namespace nests under `Mediaway.Device` (C#'s
enclosing-namespace lookup makes the child namespace `Mediaway.Device.Camera`
reachable as a bare identifier, and the compiler resolves `Camera.Open(...)`
as "navigate into the namespace" instead of "call the class", CS0234). Hit
for real in this repo's own `Mediaway.Device.Tests` project; fixed by
renaming that test project's namespace to not nest under `Mediaway.Device`
(`MediawayDeviceIntegrationTests`) rather than renaming the shipped
`Camera` class — the collision only triggers for a consumer who chooses to
nest their own code under `Mediaway.Device.*`, which no real third-party
consumer would do. Documented on the test csproj so it isn't rediscovered.
The existing Unity `CameraToTextureSample.cs` had already worked around the
*unrelated* `UnityEngine.Camera` bare-name collision with a
`using MediawayCamera = ...` alias — that alias needed updating to point at
the fully-qualified class (`Mediaway.Device.Camera.Camera`), not the
namespace, for the same underlying reason.

### Desktop specifics (new capability, not a port)

- `GpuDeviceHandle`/`GpuBufferHandle` are public, blittable structs
  (`[StructLayout(LayoutKind.Sequential)]`) used directly as P/Invoke
  parameter/field types — no separate internal wrapper, since nothing about
  them needs hiding (a raw handle passthrough, same posture the native ABI
  itself takes). The caller supplies and owns the `ID3D11Device*`; this
  binding never constructs or frees one — building the device itself
  (Vortice.Direct3D11, raw COM interop, …) is out of scope, matching the
  Rust ADR-0003 boundary.
- `IDesktopVideoCapture` has **no `ReadFramesAsync`** and **no
  auto-`ReleaseFrame`** on poll, unlike `Mediaway.Device.Camera.IVideoCapture`
  — a GPU-backed frame's texture handle stays valid only until the caller
  explicitly calls `ReleaseFrame()`, and buffering several in a channel (the
  mechanism `ReadFramesAsync` is built on) would let the shared duplication
  session invalidate/overwrite a still-queued handle. This is a deliberate,
  narrower interface than Camera's, not a missing feature.
- `DesktopVideoFrame.Dispose()` is a documented no-op for the `Gpu` storage
  case (nothing owned at the frame-object level — release happens via
  `IDesktopVideoCapture.ReleaseFrame` instead); it behaves exactly like
  Camera/Audio's existing `NativeOwnedMemoryManager`-backed disposal for the
  `Cpu` case.
- `ScreenRecord.cs` (example) now opens a real `DesktopScreenCapture`
  session and correctly manages the poll/release lifecycle, but its encode
  step throws `NotSupportedException` on every real frame: the real Screen
  backend is Gpu-storage only, and `Mediaway.Pipeline`'s `EncodeSession`/
  `VideoFrame` has no GPU texture input path today. Surfaced explicitly
  (per `caveats-and-clarity.md`'s spirit) rather than silently feeding empty
  CPU data into the encoder. Closing that gap is a separate, future change
  to `Mediaway.Pipeline`, not something this ADR's scope covers.

### Native callback (`DeviceHotplug.DeviceChanged`)

Subscribing to `DeviceChanged` registers `mediaway_device_hotplug_register_callback`;
unregistering the last handler calls `_unregister_callback` — no C#-managed
polling thread anywhere. Implementation:

- **net8.0**: a `[UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]`
  static method (`DeviceHotplug.NativeCallback`) passed as a raw
  `delegate* unmanaged[Cdecl]<nint, NativeDeviceEvent*, void>` function
  pointer directly in the `LibraryImport` declaration — no marshalling
  thunk, no per-registration delegate allocation.
- **netstandard2.0**: `UnmanagedCallersOnly` doesn't exist there — a single
  `static readonly` `NativeHotplugCallback` delegate wrapping the same
  static method, created once for the assembly's whole lifetime (the method
  captures no state, so one shared delegate instance is correct for every
  `DeviceHotplug` instance, not one per registration).
- **Routing back to the right instance**: `UnmanagedCallersOnly` methods
  cannot be instance methods or close over state, so `RegisterCallback`
  allocates `GCHandle.Alloc(this, GCHandleType.Normal)` and passes
  `GCHandle.ToIntPtr(...)` as `user_data`; the static callback decodes it
  via `GCHandle.FromIntPtr(userData).Target`. Freed only **after**
  `mediaway_device_hotplug_close`/`_unregister_callback` has returned (both
  block until any in-flight callback invocation and the bridging thread
  have finished) — freeing it any earlier would race a still-executing
  callback's `GCHandle.FromIntPtr` against the `Free()` call.
- **Must-not-throw enforced, not just documented**: the entire callback body
  is wrapped in `try { ... } catch { /* swallow */ }` — an exception
  escaping an `UnmanagedCallersOnly` method (or a classic-marshalled
  delegate thunk) terminates the process instead of propagating anywhere
  useful. This is the C# analog of the native header's own "must not
  unwind across the FFI boundary" contract for the Rust side; a subscriber
  whose handler throws loses that exception silently — documented on
  `DeviceChanged`, not a place this binding can safely do better.
- **Event payload copied, not retained**: `event` is borrowed, valid only
  for the call's duration (native frees it immediately after the callback
  returns) — the callback copies every field, including a deep copy of the
  device-id C string, before invoking C# subscribers.
- Verified against the real Windows backend
  (`Mediaway.Device.Tests.DeviceHotplug_DeviceChanged_RegistersAndUnregistersRealNativeCallback`):
  register → mode-exclusivity check (`PollEvent` correctly throws
  `CallbackModeActive` while a callback is live) → unregister → back to
  poll mode, no crash, no leak. No hardware was plugged/unplugged during
  the test, so event *delivery content* is unverified — only the
  registration/teardown machinery itself.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Keep one `Mediaway.Device` package, narrow each consumer's `NativeMethods.cs` P/Invoke surface instead | Doesn't achieve the goal driving `mediaway-device-ffi/adr/0004`: the unused domain's native code (Camera's Media Foundation plumbing, DXGI/WGC) stays linked into the shared native asset regardless of which C# functions a given consumer calls — this was the whole point of the Rust-side split, and a C#-only workaround would defeat it |
| C#-managed polling thread standing in for push mode (the original ADR-0017 deferral) | Explicit user ask this session: "네이티브 콜백으로 해 그리고. 성능을 위해서" (use the native callback, for performance) — an extra thread + polling interval adds latency and CPU the real native callback doesn't need |
| Rename the `Camera` class instead of the colliding test project's namespace | Bigger, more disruptive change to shipped public API for a collision that only bites a consumer who nests their own code under `Mediaway.Device.*` — an unforced, self-inflicted choice no real third-party consumer would make |
| Share `IAudioCapture`/`AudioFrame` between `Mediaway.Device.Audio` and `Mediaway.Device.Desktop` (identical shape today) | Would need a new cross-package dependency edge where none exists on the Rust side either; and risks exactly the kind of ambiguous-reference footgun this ADR already found once for `Camera` — two packages each independently usable stays safer for a consumer who references both |

## Consequences

### Positive

- A caller who only needs Microphone (or only Camera, or only Hotplug) no
  longer references a package pulling in Screen/DXGI or Camera/Media
  Foundation P/Invoke surface at all.
- Push-mode hotplug delivery has no added C#-side polling latency —
  delivery latency is bounded only by the Rust bridging thread's own
  ~50ms poll interval (`mediaway-device-ffi/src/hotplug.rs`'s
  `HOTPLUG_CALLBACK_POLL_INTERVAL`), not doubled by a second C#-side loop.
- Screen (Desktop) capture is real and usable from C# for the first time.

### Negative / Trade-offs

- Four packages (plus the base) to version/ship instead of one — real
  packaging overhead for a consumer who wants "just give me device capture"
  with no domain awareness (no such convenience package exists yet).
- `Mediaway.Device.Desktop`'s Screen path cannot be encoded end-to-end via
  `Mediaway.Pipeline` yet (see § Desktop specifics) — a real, currently-open
  gap this ADR surfaces but does not close.
- The `GCHandle`/`UnmanagedCallersOnly` callback machinery is meaningfully
  more complex than the deferred C#-thread design would have been — accepted
  because it was an explicit, reasoned request (performance), not a default.

## References

- `docs/adr/0017-csharp-binding-package-layout.md` — original single-package
  layout this ADR splits.
- `docs/adr/0018-csharp-netstandard20-unity.md` — dual-TFM shape every new
  leaf package follows unchanged.
- `crates/mediaway-device-ffi/adr/0004-domain-feature-split.md` — the Rust
  FFI split this ADR mirrors 1:1.
- `crates/mediaway-device-ffi/adr/0002-callback-event-delivery.md` — native
  callback contract (thread-safety, must-not-block/throw, borrowed-event
  lifetime) this binding's `DeviceChanged` inherits verbatim.
- `bindings/csharp/tests/Mediaway.Device.Tests/CaptureTests.cs` — real
  hardware verification (camera, microphone, poll-mode hotplug, push-mode
  hotplug registration/teardown).

ADRs are written in **English**.
