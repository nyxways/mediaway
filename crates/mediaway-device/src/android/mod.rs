//! Android capture backend (Camera2 NDK camera, `AAudio` microphone, `MediaProjection` screen).
//!
//! - [`AndroidCameraCapture::open`] — Camera2 NDK raw FFI capture (CPU frames). See
//!   [ADR-0001](adr/android/0001-camera2-ndk-native-camera-capture.md).
//! - [`AndroidMicrophoneCapture::open`] — `AAudio` blocking-read capture. See
//!   [ADR-0002](adr/android/0002-aaudio-microphone-capture.md).
//! - [`AndroidScreenCapture::open`] — `MediaProjection` + native `AImageReader`/`Surface`
//!   bridge; needs a host-app-supplied `MediaProjection`/`JavaVM` handle (see
//!   [`AndroidScreenCaptureConfig`]). See
//!   [ADR-0003](adr/android/0003-mediaprojection-jni-screen-capture.md).
//!
//! minSdkVersion **26** for this whole module — `AAudio` and the native `Surface` bridge both
//! require API 26 (differs from `mediaway-encoder::android`'s minSdk 21, a separate,
//! independently scoped decision; see ADR-0002/0003 § Decisions confirmed with the user).
//!
//! **Zero compile verification, zero real-hardware verification** — this dev environment has
//! no Android NDK toolchain; see the crate's `android` CI job (`.github/workflows/ci.yml`).

#![cfg_attr(target_os = "android", allow(unsafe_code))]
#![cfg_attr(not(target_os = "android"), deny(unsafe_code))]

use mediaway_common::{NativeHandle, Rational};

/// Parameters for opening Android screen capture.
///
/// **Not** a [`crate::desktop::DesktopVideoCaptureConfig`] (that type has no slot for a
/// foreign JNI object handle). See [`AndroidScreenCapture::open`] and the (Android-only)
/// `screencast` module docs for the required host-app consent-flow contract. Defined here
/// rather than in the `android`-only `screencast` module so the non-Android host stub can also
/// name this type — matches every other backend's config type being nameable regardless of
/// target OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AndroidScreenCaptureConfig {
    /// Raw `jobject` bits of an **already-global** JNI reference to a live
    /// `android.media.projection.MediaProjection`, obtained by the host app's own consent flow.
    /// This module takes ownership of deleting the global reference on
    /// [`AndroidScreenCapture::close`]/`Drop` — the host app must not delete it itself.
    ///
    /// **Android 14+ (API 34) single-use note**: both the consent `Intent` and the resulting
    /// `MediaProjection` are single-use — a second `open` needs a *fresh* `MediaProjection`
    /// from a fresh host-app consent flow, not this same handle again.
    pub media_projection: NativeHandle,
    /// Raw `JavaVM*` bits — from `android_activity::AndroidApp::vm_as_ptr()` if the host app is
    /// `android-activity`-based, or `JNIEnv::get_java_vm()` inside the host app's own JNI entry
    /// point otherwise. Must outlive the whole capture session.
    pub java_vm: NativeHandle,
    /// Virtual display pixel width.
    pub width: u32,
    /// Virtual display pixel height.
    pub height: u32,
    /// `createVirtualDisplay`'s `densityDpi` parameter.
    pub density_dpi: u32,
    /// `createVirtualDisplay`'s `flags` parameter — `DisplayManager.VIRTUAL_DISPLAY_FLAG_*`
    /// bits. Supplied by the host app (which has real compile-time access to those Java
    /// constants) rather than guessed here; typically `VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR`. See
    /// ADR-0003 § open questions #4 — unverified against a real device this session.
    pub flags: i32,
    /// Timestamp timebase for polled frames.
    pub time_base: Rational,
}

#[cfg(target_os = "android")]
mod camera;
#[cfg(target_os = "android")]
mod capabilities;
#[cfg(target_os = "android")]
mod jni_util;
#[cfg(target_os = "android")]
mod mic;
#[cfg(target_os = "android")]
mod screencast;

#[cfg(target_os = "android")]
pub use camera::AndroidCameraCapture;
#[cfg(target_os = "android")]
pub use capabilities::{request_permission, support};
#[cfg(target_os = "android")]
pub use mic::AndroidMicrophoneCapture;
#[cfg(target_os = "android")]
pub use screencast::AndroidScreenCapture;

#[cfg(not(target_os = "android"))]
mod host_stub;
#[cfg(not(target_os = "android"))]
pub use host_stub::{
    AndroidCameraCapture, AndroidMicrophoneCapture, AndroidScreenCapture, request_permission,
    support,
};
