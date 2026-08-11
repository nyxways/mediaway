# mediaway-ffi — GPU device factory C ABI

`mediaway-ffi`'s `device::gpu` module. C ABI over `mediaway-device`'s
`windows::{enumerate_gpu_adapters, GpuDevice}`
([`mediaway-device` ADR-0007](../../../../crates/mediaway-device/adr/0007-gpu-device-factory.md)).
Design: [`mediaway-ffi` GPU device factory work](../../../../crates/mediaway-ffi/src/device/gpu.rs).

## Why this exists

Screen capture and GPU-input encode both require a live
`mediaway_gpu_device_handle_t` with no CPU fallback (`adr/0003-gpu-handle-c-abi.md`
§4). Before this module, **no C ABI function anywhere created or discovered a GPU
device** — every Rust caller/test hand-rolled its own `D3D11CreateDevice` call
(e.g. `gpu_write_frame_smoke.rs`'s `open_shared_d3d11_device`), and a plain C/FFI
caller had no way to do that at all — every non-Rust language binding (and even
C's own examples, `capture_screen.c`/`screen_record.c`) could not reach Screen
capture at all, only demonstrate the gap. This module closes it.

## Shape

- Opaque handle: `GpuDeviceSessionHandle { poisoned: bool, inner: GpuDevice }` —
  same `poisoned`-flag/`catch_unwind` convention as every other handle in this
  crate.
- `mediaway_gpu_adapter_list()` / `mediaway_gpu_adapter_list_free()`: enumerate
  every DXGI adapter (name, VRAM, hardware-vs-software) — leaked as one
  `Box<[MediawayGpuAdapterInfo]>`, freed as one array (not per-entry), each
  entry's `name` a separately-owned `CString`.
- `mediaway_gpu_device_create()`: builds a real device from
  `MediawayGpuDeviceOptions` (adapter select — `Default` or explicit `Index`;
  `video_support`/`debug_layer` flags), mirroring `mediaway-device`'s Rust-level
  `GpuDeviceOptions` exactly.
- `mediaway_gpu_device_handle()`: reads the `mediaway_gpu_device_handle_t` bits —
  pass this into `mediaway_desktop_capture_config_screen()` or an encode config's
  `gpu_device` field.
- `mediaway_gpu_device_close()`: drops the device; every handle obtained from it
  becomes invalid the instant this returns (same contract as every other
  `*_close` in this crate — see `ffi-c-abi.md`).
- `impl From<CommonGpuDeviceHandle> for GpuDeviceHandle` (`common/gpu.rs`) is the
  **first output-direction** conversion for this type — previously only
  `to_common()` (C→Rust, input direction) existed, since every prior caller of
  this crate only ever *supplied* a device, never *received* one from it.

## Verified end to end

`crates/mediaway-ffi/tests/screen_capture_encode_bridge_smoke.rs` (hardware
test) and `bindings/c/examples/device/capture_screen.c` /
`bindings/c/examples/pipeline/screen_record.c` (plain C, link+run verified) all
prove the same chain: factory-created device → real DXGI Screen capture opened
with it → (bridge) real H.264 encode session. On this dev machine, GPU-input
H.264 encode itself gracefully skips (`UNSUPPORTED`) — a pre-existing
encoder/driver limitation shared with `gpu_write_frame_smoke.rs`, not something
this factory introduced; capture + geometry + frame polling all succeed for real.

## Deferred

Same Window-capture and Linux gaps as the rest of `device.h` — see
`ffi-c-abi.md` § Deferred. No adapter *filtering* (e.g. "only adapters with N
VRAM") — callers filter `mediaway_gpu_adapter_list()`'s output themselves.
