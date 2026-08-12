# ADR-0001: Camera2 NDK (raw `ndk-sys` FFI) for Android camera capture

- **Status**: Accepted
- **Date**: 2026-08-12
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device` (new module `mediaway-device::android`, ADR-0021 `#[cfg]`-gated
  backend — no separate `mediaway-device-android` crate)

## Context

This is the **first Android backend in `mediaway-device`** (the crate has Windows/Web/Linux
backends only). The user explicitly chose a **camera + microphone + screen** vertical slice in
one pass, not a camera+mic-only first cut, despite screen capture being the most novel/complex
domain (see ADR-0003). Per `docs/spec/crate-packaging.md`'s platform suffix table, `android` is
already a reserved module name — no new naming decision needed, only backend design.

`mediaway-encoder::android` (`adr/android/0001-ndk-amediacodec-h264-cpu-upload.md`, this same
session) already established the platform's baseline conventions this ADR set follows: `ndk`
crate (rust-mobile), `#[cfg(target_os = "android")]` module (not a crate), zero local NDK toolchain
in this dev environment (Windows host, no Android NDK — **zero `cargo check` run against any of
these three ADRs**), and an honest "as authored, unverified" posture pending a CI compile job.

Linux's four device ADRs (`adr/linux/0001`–`0004`) are the closest sibling precedent for
"multiple capture domains landing as separate small ADRs on one crate" — this ADR set mirrors
that shape: one ADR per domain (camera / mic / screen), not one combined ADR.

## Research: does `ndk`/`ndk-sys` wrap Camera2 NDK?

Verified by reading the real, cloned `rust-mobile/ndk` source (`local/vendor-ref/ndk`, already
present from the encoder work) rather than trusting memory or a web summary — this repo's
established, hard-won convention this session.

- **The safe `ndk` crate has no Camera2 wrapper.** `ndk/src/` has `audio.rs`, `media/` (`{
  media_codec, media_format, image_reader}`), `native_window.rs`, `hardware_buffer.rs`, etc. —
  **no `camera.rs`, no `media/camera.rs`**. Confirmed by `Glob`/`Grep` across the whole clone: zero
  hits for `Camera` anywhere under `ndk/src/`.
- **`ndk-sys` *does* generate raw FFI for the full Camera2 NDK surface.** `ndk-sys/wrapper.h`
  unconditionally includes `<camera/NdkCameraCaptureSession.h>`, `<camera/NdkCameraDevice.h>`,
  `<camera/NdkCameraError.h>`, `<camera/NdkCameraManager.h>`, `<camera/NdkCameraMetadata.h>`,
  `<camera/NdkCameraMetadataTags.h>`, `<camera/NdkCameraWindowType.h>`, `<camera/NdkCaptureRequest.h>`
  — and the bindgen output (`ndk-sys/src/ffi_aarch64.rs`, confirmed identical shape on the other
  3 arches) really does contain `ACameraManager`, `ACameraDevice`, `ACameraCaptureSession`,
  `ACaptureRequest`, `ACameraManager_create`, `ACameraManager_openCamera`,
  `ACameraDevice_createCaptureSession`, `ACameraDevice_close`, `ACaptureSessionOutputContainer_create`,
  the `ACameraDevice_StateCallbacks`/`ACameraCaptureSession_stateCallbacks` function-pointer
  struct shapes, etc.

### Real, previously-unflagged gap found this session: no auto-link for camera2ndk

`ndk-sys/src/lib.rs` has an explicit `#[cfg(all(feature = "…", target_os = "android"))]
#[link(name = "…")] extern "C" {}` block for **every** feature it exposes — `nativewindow` →
`libnativewindow.so`, `media` → `libmediandk.so`, `bitmap` → `libjnigraphics.so`, `audio` →
`libaaudio.so`, `sync` → `libsync.so`. **There is no `camera` feature in `ndk-sys`'s
`[features]` table at all (`audio`, `bitmap`, `media`, `nativewindow`, `sync` only), and
correspondingly no `#[link(name = "camera2ndk")]` block anywhere in `lib.rs`.** The Camera2 NDK
symbols are bindgen'd into the arch-specific `ffi_*.rs` unconditionally, but **nothing in
`ndk-sys` links `libcamera2ndk.so`** — depending on `ndk-sys`/`ndk` alone, even with every
feature enabled, does not make `ACameraManager_create` etc. resolve at link time. This crate
would need to add its own link directive (`#[link(name = "camera2ndk")]` extern block, or a
`build.rs` emitting `cargo:rustc-link-lib=camera2ndk`) to use any of these symbols — a genuine
gap this ADR's research caught, not something assumed from a summary.

## Decision

> Depend on **`ndk-sys` directly** (raw FFI, `[target.'cfg(target_os = "android")'.dependencies]`,
> same gating shape as the encoder's `ndk` dependency) for the Camera2 NDK surface — there is no
> safe wrapper to prefer here, unlike `AMediaCodec` (encoder ADR-0001) or AAudio (ADR-0002 in this
> set). Reuse the safe **`ndk`** crate's `media`-feature `ImageReader`/`NativeWindow` for the
> *output* side (frames land in an `ndk::media::image_reader::ImageReader`, whose `.window()`
> becomes the capture target `ACaptureRequest_addTarget` points at) — no reason to hand-roll
> `AImageReader` FFI when `ndk::media::image_reader` already wraps it safely (used identically in
> ADR-0003's screen-capture design).

- New module `mediaway-device::android::camera` (`AndroidCameraCapture`, implementing
  `crate::camera::CameraCapture`) owns: the `camera2ndk` link directive, `ACameraManager`
  enumeration, `ACameraDevice_open` + its async `ACameraDevice_StateCallbacks`
  (`onDisconnected`/`onError` are real device-loss/error signals, not just an open outcome —
  must map to `CaptureError::DeviceLost`/`Backend` on the *already-open* session, not only at
  `open()` time), `ACaptureSessionOutputContainer`/`ACameraOutputTarget` set up against the
  `ImageReader`'s `NativeWindow`, `ACameraDevice_createCaptureSession`, an `ACaptureRequest`
  built via `ACameraDevice_createCaptureRequest(TEMPLATE_PREVIEW)` +
  `ACaptureRequest_addTarget`, and `ACameraCaptureSession_setRepeatingRequest`.
- **Callback-driven `open()` is a new pattern for this crate.** Every existing backend's `open()`
  is synchronous (DXGI `DuplicateOutput`, MF `MFEnumDeviceSources`+`IMFSourceReader::Create`,
  V4L2 ioctls, portal's async-but-`block_on`-wrapped D-Bus calls). `ACameraManager_openCamera`
  is asynchronous and reports success/failure only via `ACameraDevice_StateCallbacks::onOpened`/
  `onError`, invoked from an NDK-owned callback thread. `open()` must block the caller (a bounded
  channel or condvar signaled from the C callback, with a timeout — no existing precedent in this
  crate to copy verbatim; closest analog is `AudioStreamDataCallback`'s callback-context pattern
  in ADR-0002, but that's a per-buffer callback, not a one-shot open-result signal).
- **First slice, camera selection**: enumerate via `ACameraManager_getCameraIdList`, no filtering
  by lens-facing/capability beyond "reports `REQUEST_AVAILABLE_CAPABILITIES_BACKWARD_COMPATIBLE`"
  — ordinal index in that filtered list is `Select::Id`, `Select::Default` = index `0`. Mirrors
  V4L2's own "ordinal index into an OS-filtered device list" shape
  (`adr/linux/0002-v4l2-camera-capture.md`).
- **Format**: request `AIMAGE_FORMAT_YUV_420_888` from the `ImageReader` — Camera2's standard,
  universally-supported capture format (unlike V4L2's driver-dependent `YUYV`/`NV12`/`YU12`
  menu). One fixed resolution this slice (no per-camera `StreamConfigurationMap` querying, no
  `ACameraMetadata` capability parsing) — a real, current scope limit, not a hidden one.

### `YUV_420_888` is not guaranteed to be `PixelFormat::Nv12` bytes

`YUV_420_888` is Android's *flexible* YUV format: plane 0 (Y) always has `pixel_stride == 1`, but
planes 1/2 (U/V) can be fully planar (`pixel_stride == 1`, `PixelFormat::I420`-shaped) **or**
semi-planar interleaved (`pixel_stride == 2`) — and even in the interleaved case, plane offset
order determines whether the layout is NV12-shaped (U-then-V) or NV21-shaped (V-then-U);
Camera2's public contract does not guarantee which a given device reports. This crate's own
`Image::plane_pixel_stride`/`plane_row_stride` (confirmed present in `ndk::media::image_reader`,
reads real per-plane values, not assumed ones) must be checked per plane at runtime: accept only
the case that provably matches `PixelFormat::Nv12` or `PixelFormat::I420`, and return
`CaptureError::Unsupported` for anything else — the same "never silently mis-read a layout we
didn't verify" rule V4L2's ADR-0002 already established for driver format substitution. A device
reporting NV21 (V-before-U) has **no supported format this slice**, since `PixelFormat::Nv21`
does not exist in `mediaway-common` yet — flagged as an open item, not silently mapped to `Nv12`.

## ZCA / typestate shape

| Existing precedent | Camera2 NDK | Difference |
|---|---|---|
| V4L2 `Device` + `mmap::Stream` kept alive in one worker-thread closure (zero `unsafe`) | `ACameraManager`/`ACameraDevice`/`ACameraCaptureSession`/`ACaptureRequest` raw pointers, each with its own `_delete`/`_close` teardown call, owned by a dedicated worker thread | First hand-rolled raw-pointer FFI resource chain in this crate's camera domain — no safe wrapper crate exists to lean on |
| MF `IMFSourceReader::ReadSample` (synchronous pull) | `setRepeatingRequest` (push, driven by the `ImageReader`'s `ImageListener`/`acquire_latest_image` poll) | Frame delivery is push-then-poll, not a blocking read call |
| `GpuBufferHandle::AndroidSurface` — deferred (encoder ADR-0001) | Same deferred variant, sourced from `Image::hardware_buffer()` (needs `api-level-26`, even though base `ImageReader`/`Image` need only `api-level-24`) | Reuses the already-declared `mediaway-common` variant — no new type-design work |

No `Box<dyn _>` planned; `AndroidCameraCapture` wraps a concrete inner session type behind
`Option<…>` (closed-after-move sentinel), matching every other backend's shape in this crate.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| JNI `android.hardware.camera2` (Java API) via `jni` | Even more callback-heavy than the NDK surface (`CameraDevice.StateCallback`/`CameraCaptureSession.StateCallback`/`CaptureCallback` as JNI-implemented interface objects) and ties this domain to a live `JNIEnv`/Activity — the same downside the encoder ADR already rejected for `AMediaCodec`, worse here because Java Camera2 has no NDK-equivalent lighter path. |
| Deprecated `android.hardware.Camera` (Camera1) via JNI | Deprecated since API 21, explicitly discouraged by Google, often a compatibility shim with worse quality/performance on modern devices. Rejected outright. |
| Defer camera to a follow-up ADR, ship mic + screen only this pass | Contradicts the user's explicit "camera + mic + screen 모두" direction for this vertical slice — not an option this ADR takes. |
| Hand-roll `AImageReader` FFI too (skip `ndk::media::image_reader`) | No reason to: the safe wrapper already exists, is already a dependency for ADR-0003's screen capture, and needs zero raw `unsafe` at this crate's call sites for the *output* side — same reasoning the encoder ADR used to prefer `ndk::media::media_codec` over raw `ndk-sys`. |

## Dependency review (`docs/conventions/deps-policy.md`)

`ndk`/`ndk-sys` themselves were already reviewed in `mediaway-encoder` ADR-0001 (same license —
`MIT OR Apache-2.0` — same maintainer, `rust-mobile`) and are not re-litigated here. What's new:

| Question | Answer |
|----------|--------|
| Need | Real — Camera2 is the only modern camera API surface on Android; no safe wrapper crate covers it (checked: neither `ndk` nor a credible third-party crate). |
| New surface | Raw `ndk-sys` struct/function FFI, no safe layer — the largest hand-written `unsafe` surface this crate will have (Windows uses the official `windows`-rs crate; Linux's V4L2/PipeWire/portal backends are all zero-`unsafe` via safe wrapper crates). |
| Link cost | **Must add our own `#[link(name = "camera2ndk")]` directive** (see § Real gap above) — `ndk-sys` does not do this for us, unlike every other NDK library it wraps. |
| Alternatives | See § Alternatives Considered. |

## ⚠️ CI verification plan — same posture as the encoder's Android ADR

**Zero `cargo check`/`clippy`/`test` run against this ADR** — this dev environment has no Android
NDK (Windows host). Recommended: extend the existing `android` job in `.github/workflows/ci.yml`
(`crates/mediaway-encoder/adr/android/0001-…`'s job) with a second lint step targeting
`mediaway-device`, gated on `mediaway-device`/`mediaway-common` being in the affected set. Per
§ Open questions below, this crate's step likely needs `cargo ndk -t arm64-v8a -p 26 …` (a
*different* `-p` API-level flag than the encoder's `-p 21` step, in the same job) — see the
shared minSdk discussion in ADR-0003 § Open questions (the strictest-requirement domain).

## Open questions (for user confirmation)

1. **Module shape**: one `mediaway-device::android` cfg module with per-domain files
   (`android/camera.rs`, `android/mic.rs`, `android/screencast.rs`, `android/capabilities.rs` —
   mirroring `mediaway-device::linux`'s shape) vs. Windows's split into 4 separate top-level cfg
   modules (`android`, `android_audio`, `android_camera`, `android_desktop`). **Recommendation:
   mirror Linux** — Windows's 4-way split is a historical artifact of pre-ADR-0021 separate
   crates being merged in place, not a deliberate shape chosen for a *new* platform; Linux's
   single-module shape is the more recent, deliberately-chosen precedent, and Android's domains
   plausibly share a `JavaVM`/`JNIEnv` attach helper (`android/jni_util.rs`) that a single module
   makes easier to place without cross-module plumbing.
2. **`camera2ndk` link mechanism**: `#[link(name = "camera2ndk")]` extern block in `camera.rs`
   itself vs. a crate `build.rs` emitting `cargo:rustc-link-lib=camera2ndk` — needs confirming
   against how `cargo-ndk`'s toolchain resolves system NDK libraries at link time (unverified,
   zero local NDK to test against).
3. **Camera selection scope**: "first camera reporting backward-compatible capability" vs. an
   explicit lens-facing preference (front/back) in `CameraCaptureConfig` — V4L2's precedent is
   pure ordinal index with no semantic filtering; Android devices almost always expose ≥2 cameras
   with a meaningful front/back distinction most callers care about.
4. **`YUV_420_888` layout handling**: accept only provably-`Nv12`/`I420`-shaped planes and reject
   (`Unsupported`) anything else this slice, or add a new `PixelFormat::Nv21` variant to
   `mediaway-common` now to widen device coverage? No real hardware to test either path against
   this session.
5. **Overall Android minSdk floor** — see ADR-0003 § Open questions (shared across all three
   ADRs in this set; Camera2 NDK itself only needs API 24, but AAudio (ADR-0002) needs 26 and the
   clean screen-capture bridge (ADR-0003) also needs 26).

## Decisions confirmed with the user (2026-08-12)

1. **minSdk floor**: **26** for `mediaway-device::android` as a whole (differs from
   `mediaway-encoder::android`'s 21 — two independently-scoped decisions). See ADR-0002/0003 for
   the AAudio/screen-capture reasoning that drove this; Camera2 NDK itself only needed 24.
2. **Module shape**: mirrors `linux`'s single-module, per-domain-file shape
   (`android/camera.rs`, `android/mic.rs`, `android/screencast.rs`, `android/capabilities.rs`) —
   not Windows' 4-way top-level module split.
3. **`camera2ndk` link mechanism**: a crate `build.rs` (`crates/mediaway-device/build.rs`)
   emitting `cargo:rustc-link-lib=camera2ndk` when `CARGO_CFG_TARGET_OS == "android"` — chosen
   over an inline `#[link(name = "camera2ndk")] extern "C" {}` block for visibility (one
   obviously-named file to check) and because it mirrors how a downstream crate conventionally
   adds an extra system library `ndk-sys` itself doesn't link.
4. **Camera selection scope**: kept as originally proposed — ordinal index into
   `ACameraManager_getCameraIdList`'s own order, no lens-facing (front/back) filtering this
   slice. Not separately re-litigated with the user; deferred as a real, documented scope limit.
5. **`YUV_420_888` layout handling**: accept the fully-planar case (`PixelFormat::I420`) and,
   additionally (beyond this ADR's original binary proposal), the semi-planar case where a
   pointer-adjacency check on `plane_data(1)`/`plane_data(2)` provably shows U immediately
   before V in memory (`PixelFormat::Nv12`) — found and implemented while writing the code, not
   pre-approved in the original open question, because rejecting *all* semi-planar devices would
   likely mean camera capture never works on real hardware (interleaved chroma is the common
   case). V-before-U (NV21-shaped) still has no supported format. See `camera.rs`'s
   `detect_pixel_format`.

## Implementation notes (2026-08-12, written alongside the code)

- **Real correction to this ADR's own § Decision**: `ACameraManager_openCamera` is a
  **synchronous** call — its `camera_status_t` return value and `*device` out-parameter are the
  full open outcome. Reading the real `ndk-sys` FFI while implementing `camera.rs` showed
  `ACameraDevice_StateCallbacks` has only `onDisconnected`/`onError` fields; **no `onOpened`
  member exists**. This ADR's original text assumed an async, callback-gated open needing a
  channel/condvar bridge — that bridge was not built; `open()` is a plain synchronous call on
  the worker thread, simpler than described above. The state callbacks are wired up and used
  only for post-open disconnect/error notification (`CaptureError::DeviceLost`), matching their
  real, narrower purpose.
- Frames are pulled by polling `ImageReader::acquire_latest_image` on a fixed ~8 ms interval
  (checked against the stop flag each iteration), not via `AImageReader_setImageListener`'s
  async callback — a scope simplification made while implementing, not called out as an open
  question originally: avoids a second FFI callback layer for a first slice with zero real
  hardware to tune against anyway.
- One fixed capture resolution (1280×720) this slice, as originally scoped.
- `CameraResources` (the raw FFI resource chain: manager/device/session/request/output
  target/output container/session output/`ImageReader`/boxed device-state context) is one
  `Drop`-guarded struct so every early-return error path during setup tears down whatever
  already succeeded, in reverse order — no new pattern beyond what this ADR's § ZCA table
  already anticipated ("each with its own `_delete`/`_close` teardown call").

## Consequences

### Positive

- Real device enumeration, async open, capture-session, and repeating-request plumbing — not a
  stub — reuses the safe `ndk::media::image_reader` wrapper for frame delivery instead of
  hand-rolling `AImageReader` FFI too.
- `GpuBufferHandle::AndroidSurface` already existing in `mediaway-common` means the deferred
  Zero-Copy stage (`Image::hardware_buffer()`) has no blocking type-design work left.
- Caught a real, previously-undocumented `ndk-sys` gap (no `camera2ndk` auto-link) before any
  implementation PR, not after a confusing link failure.

### Negative / Trade-offs

- **Zero compile verification as authored** — same caveat class as the encoder's Android ADR,
  compounded here by there being no safe wrapper crate to lean on for correctness.
- Largest raw-FFI `unsafe` surface this crate will carry — a real, deliberate departure from
  every other backend's "zero or wrapper-contained `unsafe`" property.
- Callback-driven `open()` is a genuinely new async-to-sync bridging pattern with no exact
  precedent elsewhere in this crate to copy.
- `YUV_420_888`'s flexible plane layout means real device coverage is unverified and
  device-dependent — cannot be resolved without real hardware.

## References

- [`docs/conventions/deps-policy.md`](../../../../docs/conventions/deps-policy.md)
- [`docs/spec/crate-packaging.md`](../../../../docs/spec/crate-packaging.md) — `android` platform
  suffix (already reserved)
- `mediaway-device` [ADR-0021](../../../../docs/adr/0021-workspace-consolidation.md) — `#[cfg]`-gated
  backend modules, no separate platform crate
- `adr/linux/0002-v4l2-camera-capture.md` — ordinal-index device selection + "never silently
  mis-read a layout" precedent this ADR mirrors
- `mediaway-encoder` `adr/android/0001-ndk-amediacodec-h264-cpu-upload.md` — binding-choice
  methodology, CI verification plan shape, honesty-about-zero-verification precedent
- [`ndk`/`ndk-sys` on crates.io](https://crates.io/crates/ndk) ·
  [GitHub (`rust-mobile/ndk`)](https://github.com/rust-mobile/ndk) (`MIT OR Apache-2.0`) — cloned
  into `local/vendor-ref/ndk` this session
- [NDK Camera2 reference](https://developer.android.com/ndk/reference/group/camera) ·
  [`NdkCameraManager.h` source](https://android.googlesource.com/platform/frameworks/av/+/master/camera/ndk/include/camera/NdkCameraManager.h)
