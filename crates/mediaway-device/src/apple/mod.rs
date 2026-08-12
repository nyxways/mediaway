//! Apple capture backend.
//!
//! `AVFoundation` camera + mic, `ScreenCaptureKit` macOS screen, `ReplayKit` iOS screen — one
//! module covering both macOS and iOS per `docs/spec/crate-packaging.md`'s `apple` platform
//! suffix.
//!
//! - [`AppleCameraCapture::open`] — `AVCaptureSession` + a `define_class!` delegate (CPU NV12
//!   frames, `VideoRange`). See [ADR-0001](adr/apple/0001-avfoundation-camera-capture.md).
//! - [`AppleMicrophoneCapture::open`] — `AVAudioEngine` input tap. See
//!   [ADR-0002](adr/apple/0002-avaudioengine-microphone-capture.md).
//! - macOS [`AppleScreenCapture::open`] (`screencapturekit` module) — `ScreenCaptureKit`
//!   `SCStream`. See [ADR-0003](adr/apple/0003-screencapturekit-macos-screen-capture.md).
//! - iOS [`AppleScreenCapture::open`] (`replaykit` module) — `RPScreenRecorder` in-app capture,
//!   **plus** [`AppleBroadcastExtensionCapture`] (a Broadcast Upload Extension push-sink — needs
//!   a real host-project `.appex` extension target this crate cannot build itself). See
//!   [ADR-0004](adr/apple/0004-replaykit-ios-inapp-screen-capture.md).
//!
//! **`AppleScreenCapture`'s `open()` signature genuinely differs by OS** (`&DesktopVideoCaptureConfig`
//! on macOS vs. parameterless on iOS, since `RPScreenRecorder` is a config-less singleton) — a
//! real, deliberate API difference, not an oversight; see each ADR's own § Decision.
//!
//! **Zero compile verification** — this dev environment cannot cross-compile Apple code at all
//! outside macOS/Xcode; see the crate's `apple-macos`/`apple-ios` CI jobs
//! (`.github/workflows/ci.yml`).

#![cfg_attr(target_vendor = "apple", allow(unsafe_code))]
#![cfg_attr(not(target_vendor = "apple"), deny(unsafe_code))]

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod camera;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod capabilities;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod mic;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod pixel;
#[cfg(target_os = "ios")]
mod replaykit;
#[cfg(target_os = "macos")]
mod screencapturekit;

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use camera::AppleCameraCapture;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use capabilities::{request_permission, support};
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use mic::AppleMicrophoneCapture;
#[cfg(target_os = "ios")]
pub use replaykit::{AppleBroadcastExtensionCapture, AppleScreenCapture};
#[cfg(target_os = "macos")]
pub use screencapturekit::AppleScreenCapture;

// `AppleBroadcastExtensionCapture` has no non-Apple stub — its only real method,
// `push_sample_buffer`, takes a `&CMSampleBuffer`, an Apple-only type with no meaningful
// off-Apple stand-in; unlike every other type here, there is no honest way to expose a
// same-shaped stub. It is iOS-only even among real Apple targets (see `replaykit` module docs).
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
mod host_stub;
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
pub use host_stub::{
    AppleCameraCapture, AppleMicrophoneCapture, AppleScreenCapture, request_permission, support,
};
