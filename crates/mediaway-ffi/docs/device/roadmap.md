# mediaway-device-ffi — roadmap

C ABI facade over `mediaway-device`. Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 1 — Surface design

- [x] ADR: opaque handle types, config structs, an 11-value status enum,
      function naming, memory ownership, header layout (`adr/0001-*.md`)
- [x] Scaffold: `Cargo.toml` (`cdylib`/`staticlib`/`rlib`), `video`/`audio`
      feature split

### 2 — Video capture surface (Camera)

- [x] Opaque `VideoCaptureHandle`: open (Camera only; Screen/Window
      deterministically `UNSUPPORTED`), geometry, poll_frame, release_frame,
      close
- [x] Hand-written `include/mediaway/device.h`

### 3 — Audio capture surface (Microphone / Loopback / ProcessLoopback)

- [x] Opaque `AudioCaptureHandle`: open, format, poll_frame, close

### 4 — CI + consumer smoke test

- [ ] CI builds cleanly
- [x] `bindings/c/examples/camera_record.c` corrected against the real ABI and
      link+run-verified (`x86_64-pc-windows-gnu`, no extra system `-l` flags
      needed beyond `-lmediaway_device_ffi -lmediaway_pipeline_ffi`): opened
      the real "WeVO WV-1080" USB camera at 1920x1080, opened the microphone
      at 48000 Hz / 1 channel, produced a real `out_camera.mp4`. Also
      surfaced a real cross-header compile hazard: `<mediaway/device.h>` and
      `<mediaway/pipeline.h>` cannot both be `#include`d directly in one
      translation unit (duplicate `mediaway_rational_t`/`mediaway_pixel_format_t`
      tag+body redefinition — a hard C error, not just the C++-only risk
      `adr/0001-capture-c-abi.md` §7 flagged); worked around in the example
      file itself pending a shared `mediaway-common-ffi` header (§ Deferred).
      See `docs/ai/wiki/device/ffi-c-abi.md`.
- [x] `bindings/c/examples/screen_record.c` corrected to match reality: it no
      longer pretends to record. It link+run-verified to the one outcome the
      real ABI actually produces — `mediaway_video_capture_open` on a
      Screen-kind config returns `MEDIAWAY_DEVICE_STATUS_UNSUPPORTED`, and the
      example exits gracefully with that explanation.

### 5 — Hotplug event delivery (callback + poll)

- [x] ADR: callback-registration + poll dual-mode C ABI for `DeviceHotplug`,
      mode exclusivity, bridging-thread mechanism, thread-safety contract,
      Python/GIL findings (`adr/0002-callback-event-delivery.md`, Accepted)
- [x] Implementation: `HotplugHandle` (`src/hotplug.rs`), the six
      `mediaway_device_hotplug_*` symbols + `mediaway_device_hotplug_callback_fn`,
      `mediaway_device_kind_t`/`mediaway_device_event_t`/`mediaway_device_event_kind_t`
      (`src/types.rs`), two new `mediaway_device_status_t` variants, default-on
      `hotplug` Cargo feature, hand-written header addition
      (`include/mediaway/device.h`) — link+run-verified
      (`x86_64-pc-windows-gnu`): every declared function resolves and behaves
      per its doc comment. 16 sibling unit tests (`src/hotplug_tests.rs`)
      against a mock `DeviceHotplug`, all passing.
- [x] **Real Windows backend wiring — done, via a redesign, not the originally
      flagged `Send` fix.** `WindowsDeviceHotplug: Send` was confirmed
      **unsound** (not just "not yet proven," ADR-0002's 2026-07-31
      Send-question-resolution addendum: the real `enumerator` field fails a
      live `IAgileObject` `QueryInterface`). ADR-0002's "lazy, thread-owned
      construction" revision replaces eager `Arc<Mutex<Box<dyn DeviceHotplug +
      Send>>>` construction with per-mode-transition construction directly on
      the thread that will own the result, so `Send` is never required at
      all. `open_hotplug` now dispatches to the real
      `mediaway_device_windows::WindowsDeviceHotplug::open` on Windows
      (Linux stays `NoBackend` — no backend exists there). 19 sibling unit
      tests pass (`src/hotplug_tests.rs`), plus a real-hardware check
      (`open_hotplug_real_windows_backend_wires_through_or_skip`) confirming
      `open()` -> `poll_event()` genuinely reach and succeed against the real
      backend.
- [x] **`close()` on a real (non-mock) `WindowsDeviceHotplug` used to crash the
      process (`STATUS_ACCESS_VIOLATION`) — root-caused and fixed.**
      Root-caused via a Win32 SEH exception filter (temporary diagnostic)
      resolving the fault address against the test binary's PDB
      (`llvm-symbolizer`), pinpointing the crash inside
      `IMMDeviceEnumerator::UnregisterEndpointNotificationCallback`:
      `WindowsDeviceHotplug::open()` called `CoUninitialize()` (via a
      function-local `ComGuard`) before returning, while `close()` later
      called through the `IMMDeviceEnumerator` obtained under that now
      torn-down apartment. It only failed to reproduce in
      `mediaway-device-windows`'s own test binary because that test's
      thread happened to have an unrelated, pre-existing COM refcount
      keeping the apartment alive by accident. Fixed in
      `mediaway-device-windows/src/hotplug.rs`: `HotplugSession` now owns
      the `ComGuard` for the object's whole lifetime (`open()` through
      `close()`), not two independent per-call scopes. Verified: this
      crate's real-hardware test now calls `close()` for real (no longer
      leaks the handle) and passes, both in isolation
      (`--test-threads=1`) and in the default parallel suite. Full
      write-up: ADR-0002's 2026-07-31 addenda.

### 6 — GPU device/buffer handles across the ABI (unblocking Screen)

- [x] ADR: `mediaway_gpu_device_handle_t`/`mediaway_gpu_buffer_handle_t`
      (`mediaway-common-ffi`, second shared type family after ADR-0015's
      `Rational`/`CodecKind`), Screen dispatch, `storage_kind`-tagged frame
      output, `mediaway_video_capture_poll_frame_blocking` (session-scoped,
      does not close), Camera-only `mediaway_video_capture_capture_once`, new
      `TIMEOUT` status (`adr/0003-gpu-handle-c-abi.md`)
- [x] Found and fixed a real Rust-level bug ahead of implementing this ADR:
      `mediaway_device::capture_video_once` closed the session before
      returning the captured frame — for Screen's GPU storage this could
      dangle the just-captured handle on a solo/last shared-session close.
      Fixed in `mediaway-device/src/video.rs`; hardware-verified in
      `mediaway-device-windows`
      (`capture_video_once_screen_is_unsupported_for_gpu_storage_or_skip`).
- [x] Implementation: Screen dispatch wired into `mediaway_video_capture_open`
      (real `WindowsScreenCapture::open`, `screen_select`,
      `open_screen_capture`); `MediawayDeviceVideoFrame` gains
      `storage_kind`/`gpu_buffer`; `gpu_device` field + enforcement
      (`INVALID_INPUT` on a Camera/Screen ⇄ `gpu_device` mismatch); ABI
      version bumped to 1 (`include/mediaway/device.h`). 8 pure-logic unit
      tests (`mediaway-common-ffi/src/gpu_tests.rs`) + 4
      (`mediaway-device-ffi/src/video_tests.rs`, enforcement rules — no
      hardware needed, every case rejected before any backend/COM call).
      `cargo check --workspace` and `clippy --all-targets --all-features -D
      warnings` clean across all four touched crates.
- [ ] `bindings/c/examples/screen_record.c` predates this ADR's
      `gpu_device` parameter on `mediaway_video_capture_config_screen` — not
      yet updated to actually exercise the new Screen path.
- [ ] Real-hardware round-trip of the new C-facing
      `mediaway_video_capture_poll_frame_blocking`/`_capture_once` functions
      themselves (as opposed to the underlying Rust calls they wrap, which
      are already hardware-verified) — not exercised via a dedicated FFI test
      this pass; would need a `windows` dev-dependency in this crate.

### Deferred (not this crate's first pass)

- ~~`WindowsDeviceHotplug: Send`~~ — resolved by design (ADR-0002's lazy,
  thread-owned construction revision sidesteps the requirement entirely
  instead of fixing it).
- ~~Screen capture GPU-handle C representation~~ — resolved by
  `adr/0003-gpu-handle-c-abi.md` (Stage 6). Window capture remains blocked —
  needs a separate native `HWND` C input shape, unaffected by that ADR.
- `mediaway-device-linux` hardware verification — the `#[cfg(target_os =
  "linux")]` dispatch arm compiles against real source but is untested on
  this (Windows) development machine
- Capability / permission probe (`mediaway_device::capability`) — separate
  Rust surface, own ADR
- `cbindgen` migration — [`docs/adr/0016-cbindgen-ffi-headers.md`](../../../docs/adr/0016-cbindgen-ffi-headers.md)
  decided to adopt cbindgen starting with this crate specifically, but its
  hand-written `include/mediaway/device.h` was already implemented and
  hardware-link-verified before that ADR concluded (parallel drafting). Not
  yet migrated — this crate's own first concrete `cbindgen` migration target,
  tracked here, not silently dropped.
- Shared `mediaway-common-ffi` header text (not just the Rust-side value
  types already unified per ADR-0015) — would resolve the real
  `device.h`+`pipeline.h` co-include hazard found in Stage 4 above; deferred
  alongside the `cbindgen` question since both bear on the same header-owns-
  the-types decision.
