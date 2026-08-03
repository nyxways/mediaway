# ADR-0002: GPU frame input C ABI — `gpu_device` reachable from C

- **Status**: Accepted
- **Date**: 2026-08-01
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-ffi`

## Context

`adr/0001-auto-encode-c-abi.md` §1 deferred `gpu_device`/`max_path_class`/`backend`
because "no `GpuBufferHandle`-equivalent C ABI type exists yet" — every session
opened from C ran the CPU-upload path, never Zero-Copy, even on a machine where
the Rust layer underneath (`mediaway`, `mediaway-encoder`,
`mediaway-encoder-windows`) already supports GPU input end-to-end
(hardware-verified in `crates/mediaway/tests/screen_mic_av_smoke.rs`).

That C ABI type now exists: `mediaway-device-ffi/adr/0003-gpu-handle-c-abi.md`
solved "a live GPU handle across a C ABI" for Screen capture's *output* (polled
frame) direction, and explicitly named this crate's deferred `gpu_device` as "the
real, intended future beneficiary — not designed here." The C# binding built on
top of this crate's ABI inherited the same gap: `Mediaway.Pipeline.EncodeSession`
had no way to accept a GPU-backed frame, so the Screen-capture example
(`bindings/csharp/examples/ScreenRecord.cs`) had to throw
`NotSupportedException` instead of encoding what it captured.

`AutoVideoEncodeConfig::max_path_class` already defaults to `CpuUpload`, whose own
doc comment states it permits `ZeroCopy`/`GpuCopy` as well — only `Readback`/
`Software` require deliberately *raising* it. This means `gpu_device` alone is
enough to reach the GPU path from C; `max_path_class`/`backend` do not need to
move to close this gap.

`mediaway-encoder-windows::AutoVideoEncoder::open` branches on `config.gpu_device`
**at open time** (`VideoInputPreference::ZeroCopyGpu` vs `CpuUploadOk`), so
`gpu_device` belongs on the config struct (open-time), not as a per-frame
parameter.

## Decision

> Extend `mediaway_auto_video_encode_config_t` with `gpu_device`
> (`mediaway_gpu_device_handle_t`) and `mediaway_video_frame_t` with
> `storage_kind`/`gpu_buffer`, mirroring `mediaway-device-ffi/adr/0003`'s shapes
> and conventions as closely as the opposite ownership direction allows.
> `max_path_class`/`backend` stay deferred.

### 1. Shared GPU mirror types, reverse direction added

`mediaway-common-ffi::gpu` already had `GpuDeviceHandle::to_common()` (C→Rust) and
`From<CommonGpuBufferHandle> for GpuBufferHandle` (Rust→C, output-only — the only
direction `mediaway-device-ffi` ever needed, since a `GpuBufferHandle` there is
always a poll result). This crate needs the **reverse** buffer conversion too,
since a GPU frame here is *input*: added `GpuBufferHandle::to_common(self) ->
Option<CommonGpuBufferHandle>`, mapping all 7 variants back, with the same
"zero/malformed native field → `None`" contract `GpuDeviceHandle::to_common`
already established. Round-trip unit tests mirror the existing `to_common_*`/
`from_common_*` style in `gpu_tests.rs`.

`mediaway-ffi::types` re-exports `GpuDeviceKind`/`GpuDeviceHandle`/
`GpuBufferKind`/`GpuBufferHandle` from `mediaway-common-ffi::gpu` under the
`Mediaway*` prefix, same as `mediaway-device-ffi` does — not redefined locally.
The C header (`include/mediaway/pipeline.h`) declares the typedefs textually
again, matching the existing accepted duplicate-typedef precedent
`mediaway_rational_t`/`mediaway_pixel_format_t` already carry (both crates'
Rust sides share one definition; C headers have no `#include`-across-crates
convention yet).

### 2. Config field: `gpu_device`

`mediaway_auto_video_encode_config_t` gains `gpu_device: mediaway_gpu_device_handle_t`.
`MEDIAWAY_GPU_DEVICE_NONE` (the zero value, matching the struct's existing
zero-init-safe convention) keeps today's CPU-only behavior — this is a
non-breaking *default* even though it is a breaking *layout* change (see §5).
`mediaway_auto_video_encode_config_new`/`_h264` set it to the zero value; callers
opt in by setting `.gpu_device` on the returned value before calling
`mediaway_auto_encoder_open`, same ergonomic shape `mediaway_auto_video_encode_config_new`
already uses for `bitrate_bps`/`pixel_format`.

Ownership: caller-owned, must outlive the `mediaway_auto_encoder_open` call —
identical contract to `mediaway-device-ffi`'s Screen-capture `gpu_device` input
(`adr/0003-gpu-handle-c-abi.md` §2). `mediaway_auto_encoder_open` converts it with
`GpuDeviceHandle::to_common()` and assigns straight into
`AutoVideoEncodeConfig::gpu_device` — no new validation beyond what
`to_common()`'s `None`-on-malformed-input already provides (a malformed handle is
silently treated as "no device", same as the config type's own zero-init-safe
philosophy elsewhere).

### 3. Frame field: `storage_kind` + `gpu_buffer` (flat struct, not a C union)

`mediaway_video_frame_t` gains `storage_kind: mediaway_video_frame_storage_kind_t`
(`CPU = 0` / `GPU = 1`) and `gpu_buffer: mediaway_gpu_buffer_handle_t`, mirroring
`mediaway-device-ffi`'s `MediawayVideoFrameStorageKind`/`MediawayDesktopFrame`
idiom exactly: a kind field decides which of two storage fields matters, instead
of a second write function or a C union. `CPU` keeps `raw_bytes`/`raw_bytes_len`
meaningful (as today) and zeroes `gpu_buffer`; `GPU` keeps `gpu_buffer` meaningful
and requires `raw_bytes == NULL`/`raw_bytes_len == 0`.

**Ownership direction is the opposite of `mediaway-device-ffi`'s identically-named
concept**: there, `gpu_buffer` is a borrowed *output* aliasing the capture
session's own texture, valid until `release_frame`. Here, `gpu_buffer` is a
borrowed *input* aliasing the **caller's** own texture, valid only for the
duration of the `mediaway_encode_session_write_frame` call — this library never
retains it past that call, never calls `Release()` on it, and does not extend its
lifetime the way a capture session extends its own polled texture's lifetime
until `release_frame`. This is a narrower, simpler hazard surface than the
capture-output case: no read-window spanning multiple calls, no "stale texture
after release" failure mode — just "valid for one synchronous call," the same
contract CPU `raw_bytes` already carries.

`mediaway_encode_session_write_frame` branches on `storage_kind`: `Cpu` keeps the
existing `borrow_slice` + `Bytes::copy_from_slice` path unchanged;
`Gpu` converts `gpu_buffer` via `GpuBufferHandle::to_common()` and builds
`VideoFrameStorage::Gpu(handle)` — a `None` result (malformed handle) returns
`MEDIAWAY_PIPELINE_STATUS_INVALID_ARGUMENT`, reusing the existing status rather
than adding a new variant (same reuse `mediaway-device-ffi` made for its own
GPU-handle validation).

### 4. No new status variant

`MEDIAWAY_PIPELINE_STATUS_INVALID_ARGUMENT` (null pointer / malformed handle) and
`MEDIAWAY_PIPELINE_STATUS_INVALID_INPUT` (bad frame metadata) already cover every
new failure mode a GPU frame introduces. No 13th status value needed.

### 5. ABI version bump: `0` → `1`

Both `mediaway_auto_video_encode_config_t` and `mediaway_video_frame_t` gain
trailing fields — a breaking `repr(C)` layout change for any consumer that
memcpys or `sizeof()`s these structs, even though existing field offsets are
unchanged (new fields are appended, not inserted). `MEDIAWAY_PIPELINE_FFI_ABI_VERSION`
and the runtime `mediaway_pipeline_ffi_abi_version()` both move to `1`, mirroring
the exact consequence `mediaway-device-ffi/adr/0003` already accepted for its own
config/frame struct growth.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Also expose `max_path_class`/`backend` now | Out of scope for closing this specific gap — `max_path_class`'s Rust-level default (`CpuUpload`) already permits the GPU path; adding the field would be scope creep with no behavior it unlocks that `gpu_device` alone doesn't already unlock. |
| A second `mediaway_encode_session_write_gpu_frame` function instead of a `storage_kind` tag | Rejected for the same reason `mediaway-device-ffi` rejected it for its poll output: doubles the function surface for two structurally similar calls, and the flat-struct-plus-discriminant idiom is now the established convention across both `-ffi` crates for a two-shape frame. |
| A C union (`data` vs `gpu_buffer` sharing memory) | `mediaway-device-ffi` already rejected this — C unions are not `#[repr(C)]`-friendly to auto-generate from Rust enums without `bindgen`/`cbindgen` union support this workspace doesn't use yet, and the flat-struct waste (a few unused bytes when the other branch applies) is negligible next to a frame's own pixel payload. |
| Move `GpuDeviceKind`/`GpuBufferKind`/handles into a shared header included by both `-ffi` crates | No cross-crate `#include` convention exists in this workspace's hand-written headers yet; duplicate textual typedefs is the already-accepted precedent (`mediaway_rational_t`, `mediaway_pixel_format_t`). Revisit if a third `-ffi` crate needs the same types. |

## Consequences

### Positive

- The real gap (`ScreenRecord.cs`'s `NotSupportedException` on GPU frames) has an
  actual fix path: Rust-level Zero-Copy GPU encode was already complete and
  hardware-verified; only the C ABI reachability was missing, and this ADR closes
  exactly that.
- Mirrors an already-solved, already-reviewed precedent almost mechanically —
  low design risk, consistent conventions across both `-ffi` crates.
- CPU-only callers are unaffected: `MEDIAWAY_GPU_DEVICE_NONE`/`storage_kind = CPU`
  reproduce today's exact behavior byte-for-byte.

### Negative / Trade-offs

- ABI version bump (`0` → `1`) — any prebuilt consumer must recompile/relink
  against the new struct layouts before upgrading.
- `backend`/`max_path_class` remain unreachable from C — a caller cannot force
  `Readback`/`Software` or pin a specific vendor SDK from this ABI. Still a real,
  documented gap, just a smaller one than before this ADR.
- Duplicate GPU-handle typedefs across two headers (`device.h`, `pipeline.h`) must
  be kept in sync by hand — same maintenance cost `mediaway_rational_t` already
  carries, now doubled for the GPU types.

## References

- `mediaway-device-ffi/adr/0003-gpu-handle-c-abi.md` — the mirrored precedent for
  every design choice above.
- `adr/0001-auto-encode-c-abi.md` §1 — the original deferral this ADR partially
  resolves.
- [`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md),
  [`docs/spec/zero-cost-abstractions.md`](../../../docs/spec/zero-cost-abstractions.md)
- `crates/mediaway/tests/screen_mic_av_smoke.rs` — the Rust-level
  real-hardware proof this ADR's C surface now reaches.
- `crates/mediaway-encoder/src/auto.rs` (`AutoVideoEncodeConfig`),
  `crates/mediaway-encoder-windows/src/auto.rs` (`AutoVideoEncoder::open`).

ADRs are **English**. Numbering is local to this `adr/` folder.
