# mediaway-device::android — ADRs

| ID | Title | Status |
|----|-------|--------|
| [0001](0001-camera2-ndk-native-camera-capture.md) | Camera2 NDK (raw `ndk-sys` FFI) for Android camera capture | Proposed |
| [0002](0002-aaudio-microphone-capture.md) | AAudio (`ndk::audio`) for Android microphone capture | Proposed |
| [0003](0003-mediaprojection-jni-screen-capture.md) | `MediaProjection` + JNI host-app handoff for Android screen capture | Proposed |

First Android backend for this crate — camera + microphone + screen capture landed as one
vertical slice, mirroring `adr/linux/`'s one-ADR-per-domain shape. Each ADR is **as-authored,
zero `cargo check`/`clippy`/`test` verified** (this dev environment has no Android NDK) and
carries its own § Open questions for user confirmation before implementation — see each ADR's
"Decisions confirmed with the user" section once filled in.

Sibling precedent for methodology (binding choice, CI-verification honesty, `#[cfg]`-module
shape): `crates/mediaway-encoder/adr/android/0001-ndk-amediacodec-h264-cpu-upload.md`.

Template: copy from [`mediaway-device/adr/template.md`](../../mediaway-device/adr/template.md).

Crate-local only. Workspace ADRs: [`docs/adr/`](../../../docs/adr/).
