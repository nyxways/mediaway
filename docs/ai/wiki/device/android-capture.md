# Android capture (camera + mic + screen) — implemented, zero compile verification until CI

- Module: `mediaway-device::android`, `cfg(target_os = "android")`, one module with
  domain-split files (`camera.rs`/`mic.rs`/`screencast.rs`/`capabilities.rs`/`jni_util.rs`),
  mirroring `linux`'s shape. minSdk **26** for this whole module (differs from
  `mediaway-encoder::android`'s minSdk 21 — separately scoped).
- ADRs (all **Accepted**):
  [`adr/android/0001`](../../../../crates/mediaway-device/adr/android/0001-camera2-ndk-native-camera-capture.md) (camera) ·
  [`0002`](../../../../crates/mediaway-device/adr/android/0002-aaudio-microphone-capture.md) (mic) ·
  [`0003`](../../../../crates/mediaway-device/adr/android/0003-mediaprojection-jni-screen-capture.md) (screen).
- **Zero compile verification, zero real-hardware verification** — no Android NDK toolchain in
  this dev environment; the `android` CI job (`.github/workflows/ci.yml`) now lints both
  `mediaway-encoder` (`-p 21`) and `mediaway-device` (`-p 26`) for `arm64-v8a`.

## Camera (`AndroidCameraCapture`, Camera2 NDK raw `ndk-sys` FFI)

No safe wrapper crate exists for Camera2 (`ndk` has none). `ndk-sys` bindgens the full raw FFI
but has **no `#[link(name = "camera2ndk")]` directive** — this crate's own `build.rs` emits
`cargo:rustc-link-lib=camera2ndk` to close that gap. Real correction found while implementing:
`ACameraManager_openCamera` is **synchronous** (`ACameraDevice_StateCallbacks` has only
`onDisconnected`/`onError`, no `onOpened`) — `open()` needs no async channel/condvar bridge.
Frames are pulled by polling `ImageReader::acquire_latest_image` (~8 ms interval), one fixed
1280×720 resolution. `YUV_420_888` layout: `PixelFormat::I420` (fully planar) or `Nv12`
(semi-planar, provable via a `plane_data(1)`/`plane_data(2)` pointer-adjacency check — V-before-U
NV21-shaped devices have no supported format yet).

## Mic (`AndroidMicrophoneCapture`, AAudio via `ndk::audio`)

Blocking `read()` on a dedicated worker thread, **not** AAudio's `data_callback` model — the
callback's own docs forbid taking a mutex inside it, which this crate's shared
`Arc<Mutex<VecDeque<AudioFrame>>>` queue shape needs. `ndk`'s `audio` feature bakes in
`api-level-26` (`audio = ["ffi/audio", "api-level-26"]`) — the real reason this module's minSdk
is 26, not 21. System default input only (`device_id(0)`); `F32` PCM only.

## Screen (`AndroidScreenCapture`, `MediaProjection` + JNI)

The hardest domain. `MediaProjection` only exists as a Java object obtained through a consent
flow (`createScreenCaptureIntent` → `startActivityForResult` → `onActivityResult` →
`getMediaProjection`) that only a JVM `Activity` can run — `android-activity`'s stock `AndroidApp`
has no activity-result hook at all, confirmed via its real source. The host app must ship a
custom `Activity` subclass, run that consent flow, convert the result to a JNI **global**
reference, and hand the raw bits + a `JavaVM*` to `AndroidScreenCaptureConfig`
(`mediaway-device::android::AndroidScreenCaptureConfig`, defined in `android/mod.rs` so the
non-Android host stub can also name it). From there **Rust owns everything**: native
`ImageReader` → `.window()` → `ANativeWindow_toSurface` → one `createVirtualDisplay` JNI call
(no per-frame JNI cost) → poll loop → `close()` → `mediaProjection.stop()` (JNI) → `Global`'s
`Drop` deletes the reference.

Two different `jni-sys` crate major versions are bridged via raw pointer casts: `ndk` 0.9 pins
`jni-sys 0.3` (`NativeWindow::to_surface`'s signature); this crate's `jni = "0.22"` is built on
`jni-sys 0.4`. Both model the same fixed JNI ABI, so a `.cast()` between the two is a real,
documented bridge, not a coincidental shortcut — see `screencast.rs` module docs. `jni` 0.22.4's
newer lifetime-token `Env<'local>`/`JavaVM` API was chosen over the older, more widely
documented 0.21 shape (user's explicit choice, against this session's own initial
recommendation).

**Real Android 14+ gotcha**: both the consent `Intent` and the `MediaProjection` instance are
single-use — a second `open()` needs a *fresh* host-app consent flow, not the same handle again.
`capture_desktop_video_once`-style "just a session-open cost" framing does not hold for this
domain.

**Unclosed verification gap**: even a green CI compile cannot confirm the JNI method-signature
strings (`createVirtualDisplay`'s 8-parameter signature, `stop()`'s `()V`) are correct — a wrong
signature is a runtime `NoSuchMethodError`, not a compile error, and no Android
emulator/device exists in this workspace to catch that.

## Capabilities

`support(Camera)` does a real, cheap `ACameraManager_getCameraIdList` count (no device opened).
`support(Microphone)` reports `Supported` unconditionally — AAudio has no cheap "is a mic
present" query, unlike PipeWire's daemon-connect probe on Linux. `support(Screen)` and every
other kind report `Unavailable(NotImplemented)` — screen-capture availability can only be
determined by the host app's own JNI-attached consent flow, which this parameterless probe has
no `JavaVM`/`Env` to attempt. `request_permission(Camera)` opens a real session and observes
`ACAMERA_ERROR_PERMISSION_DENIED`; `request_permission(Microphone)` returns
`PermissionState::Unknown` (AAudio has no reliable way to distinguish a RECORD_AUDIO denial from
any other `open_stream` failure).

## Related

- [`platform/android-encode`](../platform/android-encode.md) — sibling precedent (binding
  choice, CI-verification honesty) this design mirrors
- [scaffold](scaffold.md) · [capabilities](capabilities.md) · [linux-camera](linux-camera.md)
