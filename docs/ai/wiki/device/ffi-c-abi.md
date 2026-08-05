# mediaway-ffi — Camera + Screen video, microphone audio C ABI

`mediaway-ffi`'s `device` module (formerly the standalone `mediaway-device-ffi` crate,
merged by ADR-0021). Wraps `mediaway_device::{VideoCapture, AudioCapture}` over a
hand-written C ABI. Full design:
[`crates/mediaway-ffi/adr/0001-capture-c-abi.md`](../../../../crates/mediaway-ffi/adr/0001-capture-c-abi.md),
GPU handles: [`adr/0003-gpu-handle-c-abi.md`](../../../../crates/mediaway-ffi/adr/0003-gpu-handle-c-abi.md).

## Scope

**Camera** (video, CPU-only), **Screen** (video, GPU-only, Windows), and
**Microphone / Loopback / ProcessLoopback** (audio) all ship — real,
hardware-verified Windows backends. Screen requires a live
`mediaway_gpu_device_handle_t` (`MEDIAWAY_GPU_DEVICE_DIRECTX11`) passed to
`mediaway_video_capture_config_screen()` — no CPU fallback exists in the wrapped Rust
backend. `mediaway_video_capture_open()` enforces the pairing: Camera + non-`NONE`
`gpu_device`, or Screen + `NONE`/malformed `gpu_device`, both return
`INVALID_INPUT` rather than silently ignoring the mismatch. Window capture is deferred (see below).

`mediaway_device_video_frame_t` carries a `storage_kind` tag: `CPU` (owned bytes,
Camera) or `GPU` (borrowed `mediaway_gpu_buffer_handle_t`, Screen — never freed by the
caller; COM refcount + read-window + `ID3D11Multithread` hazards are documented on the
header, `adr/0003` §8). Two new functions:
`mediaway_video_capture_poll_frame_blocking` (session-scoped, does not close — the
recommended way to grab one Screen frame) and `mediaway_video_capture_capture_once`
(Camera-**only** convenience; closing a solo/last Screen session would dangle the
just-captured GPU handle — a real bug found and fixed in the wrapped
`mediaway_device::capture_video_once` ahead of this ADR, see `adr/0003` § Context).

## Shape

- Opaque handles: `VideoCaptureHandle { poisoned: bool, inner: Box<dyn VideoCapture> }`,
  `AudioCaptureHandle { poisoned: bool, inner: Box<dyn AudioCapture> }` — trait objects
  (not a closed backend enum). Both need `poisoned` (unlike `mediaway-ffi`'s
  `AutoEncoderHandle`): `poll_frame`/`release_frame` are repeated-call APIs, same shape
  as `mediaway-ffi`'s `MuxerHandle`/`DemuxerHandle`.
- `MediawayDeviceStatus` (`#[repr(C)]`, 13 values, `Ok = 0`): fresh, distinctly-named
  from `MediawayStatus`/`MediawayPipelineStatus` — see the ADR §3. `CallbackAlreadyRegistered`/
  `CallbackModeActive` were added by `adr/0002-callback-event-delivery.md` §7.
- Modules: `status.rs`, `types.rs`, `buffer.rs` (owned-output leak/reclaim helpers only),
  `video.rs`/`audio.rs`/`hotplug.rs` (each its own Cargo feature; hotplug: see
  [`hotplug-ffi.md`](hotplug-ffi.md)), `lib.rs`. Local `#[cfg(windows)]`/
  `#[cfg(target_os = "linux")]` Camera/Screen dispatch lives inside `video.rs`,
  mirroring (not importing) `mediaway::platform`'s shape.

## Type reuse vs. new types

`mediaway_rational_t`/`mediaway_pixel_format_t`/`mediaway_sample_format_t`/GPU handle
types are all `common::types`/`common::gpu` aliases now (`adr/common/0001`) — no more
per-module copies. `mediaway_gpu_buffer_handle_t` gained its first **reverse** (C→Rust)
use, `GpuBufferHandle::to_common()`, once `pipeline` also needed it as encode *input*
(`adr/pipeline/0002-gpu-frame-input-c-abi.md`); this crate only ever produces it as poll
output. `mediaway_device_video_frame_t`/`_audio_frame_t` stay **new, crate-scoped**
names, not reused from `pipeline`'s `mediaway_video_frame_t` (borrowed input there vs.
owned output here — same collision class `adr/pipeline/0001` §4c fixed for packets).

## Panic safety, ownership, header

`catch_unwind(AssertUnwindSafe(...))` on every exported function except plain-value
config constructors; a caught panic during `poll_frame`/`release_frame` sets
`poisoned` (short-circuits to `HANDLE_POISONED`), **except** `mediaway_*_capture_close`
(always safe). CPU `poll_frame` output copies once into an owned allocation (deferred
Zero-Copy gap, shared with the other two `-ffi` crates); GPU output is a borrowed
handle, never copied. `mediaway_*_capture_close` **joins the backend's worker thread**
(blocks up to one frame/period interval — [`caveats-and-clarity.md`](../../../spec/caveats-and-clarity.md))
and returns a real status instead of `void`. Header is hand-written
(`include/mediaway/device.h`); shared value types live in `include/mediaway/common.h`,
`#include`d by `device.h`/`pipeline.h`/`container.h` — see
[`adr/common/0001-shared-header-consolidation.md`](../../../../crates/mediaway-ffi/adr/common/0001-shared-header-consolidation.md).

## Deferred

Window capture (needs a native `HWND` C input shape), status-enum + buffer-free
fragmentation across all three modules, `cbindgen` migration of this file (tooling
adopted crate-wide, this header itself not yet cut over —
[ADR-0016](../../../adr/0016-cbindgen-ffi-headers.md)'s 2026-08-05 addendum), Linux
hardware verification (untested on this Windows dev machine; `LinuxScreenCapture` is
CPU-only — a different shape than this crate's GPU-mandatory Screen dispatch, not
wired), capability/permission probe ABI. See `adr/0001`/`adr/0003`'s own § Deferred
for detail. Hotplug (`DeviceHotplug`) C ABI is implemented (ADR-0002) but blocked on
`WindowsDeviceHotplug: Send` — see [`hotplug-ffi.md`](hotplug-ffi.md).

## Building the C examples on Windows

Same GNU-target route as `mediaway-ffi`: `gcc`/MinGW can't link the default
MSVC output, so build for `x86_64-pc-windows-gnu` instead:

```
cargo build -p mediaway-ffi --target x86_64-pc-windows-gnu
gcc -Icrates/mediaway-ffi/include bindings/c/examples/device/camera_record.c \
    -Ltarget/x86_64-pc-windows-gnu/debug -lmediaway_ffi \
    -o camera_record.exe
```

`camera_record.c` links the merged lib and `#include`s `<mediaway/container.h>`,
`<mediaway/device.h>`, and `<mediaway/pipeline.h>` together in one translation unit —
co-inclusion is verified safe (`adr/common/0001-shared-header-consolidation.md`; the DLL
must still sit next to the `.exe`). Verified pre-ADR-0003: real 1920x1080 "WeVO WV-1080"
camera + 48000 Hz/1ch mic captured into `out_camera.mp4`. `screen_record.c` predates
ADR-0003's `gpu_device` parameter and needs updating to build again — not done yet.
