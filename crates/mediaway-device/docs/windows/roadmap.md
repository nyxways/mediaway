# mediaway-device-windows — roadmap

Windows DXGI screen + WGC window + WASAPI capture backend.  
Facade: [`mediaway-device`](../../mediaway-device/docs/roadmap.md).  
Platform order: **Windows first**. Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 0 — Scaffold

- [x] Workspace member + docs / ADR surface
- [x] Stubs on non-Windows → `Unsupported`

### 1 — DXGI Desktop Duplication (screen Zero-Copy)

- [x] `CaptureSource::Screen` + `WindowsScreenCapture`
- [x] `poll_frame` → `DirectX11` (`Bgra8`); `release_frame` → `ReleaseFrame`

### 2 — Audio + overlay helpers

- [x] WASAPI mic + system + process loopback
- [x] `exclude_window_from_capture`
- [x] WASAPI shared-buffer path evaluated — genuine CPU ⚡ not achievable under the
      current `AudioCapture` contract + WASAPI `GetBuffer`/`ReleaseBuffer` lifetime rules
      (facade-level change needed; out of scope here). Collapsed the per-period copy from
      zero-init + memcpy to a single write. Mark stays 🆗, not ⚡ — see ADR-0002 addendum.
- [x] [ADR-0005](../adr/0005-wasapi-playback.md): WASAPI shared-mode render
      playback — `WindowsWasapiPlayback` in `src/wasapi_playback.rs`, mirroring
      `WindowsWasapiCapture`'s `ComGuard` + bounded-queue + timer-poll worker
      shape in the opposite data direction. `write_frame` push model,
      `QueueFull(AudioFrame)` submission backpressure, silence-fill +
      `underrun_count()` on render-side underrun. Hardware-verified: opened a
      real default render endpoint and observed the timer-poll worker
      silence-fill an empty queue (`underrun_count` climbing), with zero
      audible output since no frame was ever written.

### 3 — Window capture (WGC)

- [x] `CaptureSource::Window` + `WindowsWindowCapture` (separate from screen)
- [x] Frame-pool recreate on content-size change
- [ ] Proven CI/`machine_id` ⚡ cell

### 4 — Camera

- [x] Media Foundation camera (`src/camera.rs`) — `IMFSourceReader` via
      `MFEnumDeviceSources`, native-format negotiation with a video-processor
      fallback for MJPG/YUY2-only webcams. Hardware-verified: enumerated a
      real "WeVO WV-1080" USB webcam and captured real 1920x1080 frames
      end-to-end.
- [x] CPU-copy only (no DX11 Zero-Copy path yet, unlike screen capture's ⚡)
- [x] Wired into this crate's public API (`mod camera;` + `pub use
      camera::WindowsCameraCapture;` in `lib.rs`) — `CaptureSource::Camera`
      now carries `select: Select` (ADR-0005), resolved via
      `resolve_camera_index`.

### 5 — Capability / permission probe

- [x] `capabilities::support` — live checks, not just "was Windows compiled
      in": `GraphicsCaptureSession::IsSupported` (window), DXGI adapter/output
      enumeration (screen), `WASAPI` endpoint enumeration (mic/loopback), real
      process-loopback activation attempt (`ProcessLoopback`)
- [x] `capabilities::request_permission` — real `WASAPI` open/close probe for
      `Microphone` (no cheaper OS consent-check API exists for Win32 apps);
      `Unknown` (not guessed) for screen/window, `Granted` for loopback
- [x] `HARDWARE_TEST_LOCK` — serializes real-hardware tests across
      `lib_tests.rs`/`capabilities_tests.rs`; a genuine concurrent-access
      `STATUS_ACCESS_VIOLATION` crash was observed and fixed this session

### 6 — Device selection, enumeration, `DeviceLost` ([ADR-0005](../../mediaway-device/adr/0005-device-selection.md))

- [x] `Select` resolution (`Default`/`Id`/`NameContains`) wired into
      `wasapi.rs::resolve_endpoint` (mic/loopback endpoints, reused by
      `wasapi_playback.rs`), `camera.rs::resolve_camera_index`,
      `dxgi.rs::resolve_output_index` (adapter-scoped, per ADR)
- [x] `enumerate(kind)` (`src/enumeration.rs`) — Microphone/Loopback (`WASAPI`
      `EnumAudioEndpoints` + friendly name), Camera
      (`MFEnumDeviceSources` + symbolic link `DeviceId`), Screen (DXGI
      adapters/outputs, global — not adapter-scoped like `open()`).
      `ProcessLoopback`/`Window` → `Unsupported` (never an empty `Vec`).
      Hardware-verified: real endpoint/output enumeration observed
      (`is_default`/`ordinal` asserted, not just "doesn't crash").
- [x] **Real bug found + fixed on real hardware this session**: on this
      Korean-locale Windows machine, `PKEY_Device_FriendlyName` alone is not
      always a unique per-endpoint name (a known Windows footgun — some
      drivers report only the generic localized device-class label).
      `endpoint_friendly_name` now also queries
      `PKEY_DeviceInterface_FriendlyName` and appends it in parentheses only
      when not already present in the endpoint name (verified against 4 real
      capture endpoints across 3 adapters — no duplicated suffixes, no
      collisions).
- [x] `DeviceLost` wiring — `wasapi.rs::pump_capture_loop` /
      `wasapi_playback.rs::pump_playback_loop` now set a `device_lost` flag
      (distinct from a caller-requested `stop`) on real WASAPI failures;
      `poll_frame` drains any already-buffered frames first, then reports
      `CaptureError::DeviceLost`; `write_frame` reports
      `PlaybackError::DeviceLost` immediately. Not hardware-verified against
      a real unplug event this session (would require physically
      disconnecting a device mid-test) — logic-verified via a fake-session
      unit test instead.
- [x] `WindowsDeviceHotplug` (`src/hotplug.rs`) — real `IMMNotificationClient`
      COM callback registered via
      `IMMDeviceEnumerator::RegisterEndpointNotificationCallback`, scoped to
      Microphone/Loopback only (ADR-0005 § Hotplug). `#[implement]`-generated
      COM server object (`wasapi_process.rs`'s
      `IActivateAudioInterfaceCompletionHandler` handler was this crate's
      first *provided* COM interface, but a one-shot synchronously-awaited
      callback — this is the first long-lived, OS-driven, arbitrary-MTA-
      thread one). Callbacks push into a bounded `Arc<Mutex<VecDeque<_>>>`
      queue (drop-oldest past capacity, mirroring `wasapi.rs`'s capture
      queue); `poll_event` drains it non-blockingly, no COM calls.
      `EDataFlow`/`ERole` → `DeviceKind` mapping is pure and unit-tested.
      Hardware-verified: opened a real registration, polled
      idle (`Ok(None)`) with no plug/unplug simulated, and closed cleanly.
      **2026-07-31 fix**: `open()`/`close()` originally used two independent,
      short-lived `CoInitializeEx`/`ComGuard` scopes — `open()`'s own guard
      called `CoUninitialize()` before returning, leaving the stored
      `enumerator`/`client` referencing a torn-down apartment for `close()`
      to later use through. Reproduced as a real `STATUS_ACCESS_VIOLATION`
      inside `IMMDeviceEnumerator::UnregisterEndpointNotificationCallback`
      when exercised from a different binary
      (`mediaway-device-ffi/adr/0002-callback-event-delivery.md`'s
      addenda) — this crate's own test never caught it because its calling
      thread happened to have an unrelated COM refcount keeping the
      apartment alive by accident. Fixed: `HotplugSession` now owns the
      `ComGuard` for the object's whole lifetime. Also newly documented:
      `MMDeviceEnumerator` is confirmed **not** `IAgileObject`
      (`lib_tests.rs::mmdevice_enumerator_does_not_implement_iagileobject_or_skip`),
      so `open`/`poll_event`/`close` must all run on the same thread —
      stated explicitly in `hotplug.rs`'s type-level doc.
