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
- [x] `bindings/c/examples/pipeline/screen_record.c` and
      `bindings/c/examples/device/capture_screen.c` updated to actually
      exercise the Screen path, via Stage 7's GPU device factory — see below.
- [ ] Real-hardware round-trip of the new C-facing
      `mediaway_video_capture_poll_frame_blocking`/`_capture_once` functions
      themselves (as opposed to the underlying Rust calls they wrap, which
      are already hardware-verified) — not exercised via a dedicated FFI test
      this pass; would need a `windows` dev-dependency in this crate.

### 7 — GPU device factory (unblocking non-Rust Screen capture entirely)

- [x] `mediaway_gpu_adapter_list`/`_list_free`, `mediaway_gpu_device_create`,
      `mediaway_gpu_device_handle`, `mediaway_gpu_device_close` — C ABI over
      `mediaway-device`'s `windows::{enumerate_gpu_adapters, GpuDevice}`
      (`mediaway-device` ADR-0007). Closes the last real gap for Screen
      capture from any non-Rust binding: before this, no C ABI function
      anywhere could create or discover a GPU device, so every caller (Rust
      test or FFI) had to hand-roll its own `D3D11CreateDevice`, and a
      non-Rust caller had no way to do that at all.
      `impl From<CommonGpuDeviceHandle> for GpuDeviceHandle`
      (`common/gpu.rs`) added as the first output-direction conversion for
      that type. See
      [`docs/ai/wiki/device/gpu-device-factory-ffi.md`](../../../../docs/ai/wiki/device/gpu-device-factory-ffi.md).
- [x] Capstone hardware test:
      `tests/screen_capture_encode_bridge_smoke.rs` — factory-created device
      → real Screen capture opened with it → the existing capture-to-encode
      bridge (`adr/pipeline/0005`) → fMP4. `bindings/c/examples/device/capture_screen.c`
      link+run-verified on real hardware (5 real 1920x1080 frames polled from
      plain C); `bindings/c/examples/pipeline/screen_record.c` likewise (real
      screen + mic capture; GPU-input H.264 encode itself gracefully skips on
      this dev machine's current encoder/driver, a pre-existing limitation
      shared with `gpu_write_frame_smoke.rs`, not introduced by this work).
- [x] Found and fixed an unrelated real bug while verifying: this crate's
      `mediaway_pipeline_ffi_abi_version()` still returned `5` while
      `include/mediaway/pipeline.h`'s `MEDIAWAY_PIPELINE_FFI_ABI_VERSION` had
      been bumped to `6` (adding `gop_size`/`rate_control_*` fields, #33) —
      the runtime counterpart was missed at the time. Every C example's own
      ABI-version self-check was silently failing as a result. Fixed:
      runtime now also returns `6`.

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
  decided to adopt cbindgen (written when this was still the standalone
  `mediaway-device-ffi` crate; its 2026-08-05 addendum updates the decision for
  the ADR-0021 merge). Tooling now real: `cbindgen.toml` +
  `tools/scripts/cbindgen-headers.ts` (`generate`/`verify`) produce a header
  covering the whole crate that compiles clean (gcc/g++, `-Wall -Wextra`).
  `include/mediaway/device.h` itself is **not yet migrated** — still
  hand-written, still the one hardware-link-verified and shipped; cutting it
  over means updating every `bindings/c/examples/device/*.c` file and
  re-verifying hardware, tracked here as real follow-up work, not silently
  dropped.
- ~~Shared `mediaway-common-ffi` header text~~ — resolved:
  `include/mediaway/common.h` now holds the shared value types
  (`mediaway_rational_t`, `mediaway_pixel_format_t`, GPU handle types, …),
  `#include`d by `container.h`/`device.h`/`pipeline.h`
  (`adr/common/0001-shared-header-consolidation.md`). Note: the co-include of
  `device.h`+`pipeline.h` found in Stage 4 above was already non-fatal by the
  time this was tackled (matching `#ifndef ..._T_DEFINED` guards across the
  three headers already prevented the redefinition error, verified directly)
  — the real gap this closed was the 3-way copy-pasted definitions, not an
  active compile failure.
