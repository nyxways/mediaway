# Mediaway release notes

<!-- Dev changes accumulate under ## Unreleased (AGENTS.md § 10). Finalize
     with `/release-notes <version>`; reset this template with
     `/release-notes reset`. See docs/ai/wiki/meta/release-notes.md. -->

## Unreleased

### Added

- `mediaway-encoder::android`: first Android backend (NDK `AMediaCodec` via the `ndk` crate),
  H.264 CPU-upload encode only. Zero compile verification as authored (no Android NDK in the
  dev environment) — a new CI job compiles/lints it against a real NDK before it is trusted;
  not wired into `auto`/`capability` yet. See
  `crates/mediaway-encoder/adr/android/0001-ndk-amediacodec-h264-cpu-upload.md`.
- `mediaway-encoder::apple`: last "Other" platform encoder backend (`VideoToolbox`
  `VTCompressionSession` via `objc2-*`), H.264 CPU-upload encode only, one module for both
  macOS and iOS. Zero compile verification as authored (this dev environment cannot
  cross-compile Apple code at all outside macOS/Xcode) — new `apple-macos`/`apple-ios` CI jobs
  compile/lint it against real Apple SDKs before it is trusted; not wired into
  `auto`/`capability` yet. Per-packet `is_keyframe` is a documented approximation. See
  `crates/mediaway-encoder/adr/apple/0001-videotoolbox-h264-cpu-upload.md`.
- `VideoEncoderConfig::color_range` (`ColorRange::Video`/`Full`, `mediaway-common`): configurable
  YUV sample range for encoder input. Only the Apple backend honors it so far; other backends
  accept the field without yet branching on it (documented capability-gated fallback, same
  convention as `gop_size`).
- `mediaway-device::android`: first Android device-capture backend — camera (Camera2 NDK raw
  FFI), microphone (AAudio blocking read), and screen (`MediaProjection` + JNI, with a
  documented host-app consent-flow contract). minSdk 26 (differs from
  `mediaway-encoder::android`'s 21). Zero compile verification as authored (no Android NDK in
  the dev environment) — the `android` CI job now also lints `mediaway-device` against a real
  NDK before it is trusted; not wired into any cross-platform capture-selection API yet. See
  `crates/mediaway-device/adr/android/0001-camera2-ndk-native-camera-capture.md`,
  `0002-aaudio-microphone-capture.md`, `0003-mediaprojection-jni-screen-capture.md`.

### Changed

### Fixed

- Android `mediaway-encoder::android` backend: `AMediaFormat`'s `i-frame-interval` (seconds
  between key frames) was hardcoded to `0` instead of being computed from
  `VideoEncoderConfig::gop_size`.

### Removed

### Deprecated

### Breaking
