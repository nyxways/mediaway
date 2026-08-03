//! Capability / permission probing for the Linux portal-based backend. See
//! [`mediaway-device` ADR-0003](../../mediaway-device/adr/0003-capability-and-permission-probe.md).
//!
//! **Zero runtime verification**: like the rest of this crate (see crate
//! ADR-0001/0002/0003/0004), none of [`support`]/[`request_permission`]'s
//! real-probe branches has been exercised against a real portal/compositor
//! session, `PipeWire` daemon, or V4L2 device — compile-checked only.

use crate::camera::{
    CameraCapture, CameraCaptureConfig, CaptureOutputPreference as CameraOutputPreference,
};
use crate::desktop::{
    CaptureOutputPreference as DesktopOutputPreference, DesktopCaptureSource, DesktopVideoCapture,
    DesktopVideoCaptureConfig,
};
use crate::{CaptureError, DeviceKind, PermissionState, Select, Support, Unavailable};
use mediaway_common::{NativeHandle, Rational};

use crate::linux::camera::{self, LinuxCameraCapture};
use crate::linux::mic;
use crate::linux::portal;
use crate::linux::screencast::LinuxScreenCapture;
use crate::linux::window::LinuxWindowCapture;

/// Live support probe for `kind` on this machine.
///
/// [`DeviceKind::Screen`]/[`DeviceKind::Window`] share one real D-Bus round
/// trip — connecting to `org.freedesktop.portal.Desktop` and confirming the
/// `ScreenCast` interface exists — but create no session, so they show no
/// consent dialog (unlike [`request_permission`]). [`DeviceKind::Camera`]
/// enumerates real `V4L2` capture-capable nodes (`VIDIOC_QUERYCAP`, no device
/// opened for streaming). [`DeviceKind::Microphone`] attempts a real
/// (cheap, non-streaming) `PipeWire` daemon connect. `Loopback`/
/// `ProcessLoopback` have no backend this session — see crate
/// `docs/roadmap.md`.
#[must_use]
pub fn support(kind: DeviceKind) -> Support {
    match kind {
        DeviceKind::Screen | DeviceKind::Window => screencast_portal_support(),
        DeviceKind::Camera => camera_support(),
        DeviceKind::Microphone => microphone_support(),
        // Covers Loopback/ProcessLoopback (not implemented this session) and
        // any future `DeviceKind` variant (`#[non_exhaustive]`).
        _ => Support::Unavailable(Unavailable::NotImplemented),
    }
}

/// A missing/unreachable portal is classified as [`Unavailable::NoDeviceFound`]
/// rather than [`Unavailable::OsVersionTooOld`] — portals aren't gated by
/// kernel/distro version, they're gated by which desktop session is running;
/// like a literal missing device, this can change without a rebuild (start a
/// desktop session, install `xdg-desktop-portal-gtk`, …).
fn screencast_portal_support() -> Support {
    if portal::probe_screencast().is_ok() {
        Support::Supported
    } else {
        Support::Unavailable(Unavailable::NoDeviceFound)
    }
}

/// Enumerates capture-capable `V4L2` nodes (see [`camera::enumerate_camera_paths`])
/// — cheaper than a real [`LinuxCameraCapture::open`] (no `mmap` arena, no
/// worker thread, no format negotiation).
fn camera_support() -> Support {
    if camera::enumerate_camera_paths().is_empty() {
        Support::Unavailable(Unavailable::NoDeviceFound)
    } else {
        Support::Supported
    }
}

/// Connects to the local `PipeWire` daemon socket and immediately drops the
/// connection (see [`mic::probe_daemon_reachable`]) — cheaper than a real
/// `LinuxMicrophoneCapture::open` (no stream, no format negotiation, no
/// worker thread).
fn microphone_support() -> Support {
    if mic::probe_daemon_reachable() {
        Support::Supported
    } else {
        Support::Unavailable(Unavailable::NoDeviceFound)
    }
}

/// Best-effort permission probe for `kind`.
///
/// # Cost
///
/// For [`DeviceKind::Screen`]/[`DeviceKind::Window`] this performs a **real
/// portal `ScreenCast` handshake** (`create_session` → `select_sources` →
/// `start`) — the same path [`LinuxScreenCapture::open`]/
/// [`LinuxWindowCapture::open`] take, and what actually shows the desktop's
/// screen/window-share consent dialog — then closes the session immediately.
/// For [`DeviceKind::Camera`] this **opens a real `V4L2` device** (the same
/// path [`LinuxCameraCapture::open`] takes) purely to observe whether the
/// open succeeds, then closes it — V4L2 has no separate portal-style consent
/// step; device-file permissions (the `video` group) are the only gate, and
/// opening is the only way to observe them. Callers must not call this per
/// frame; cache the result, and call [`support`] first.
///
/// [`DeviceKind::Microphone`] has no portal/consent gate on desktop
/// `PipeWire` either (unlike Windows' privacy-settings-gated microphone) —
/// reported as [`PermissionState::Granted`] once [`support`] confirms a
/// daemon is reachable, same reasoning `mediaway-device-windows`
/// `capabilities.rs` applies to render-endpoint loopback.
///
/// # Errors
///
/// Returns the underlying [`CaptureError`] when a probe itself fails for a
/// reason other than access denial.
pub fn request_permission(kind: DeviceKind) -> Result<PermissionState, CaptureError> {
    if matches!(support(kind), Support::Unavailable(_)) {
        return Ok(PermissionState::NotSupported);
    }
    match kind {
        DeviceKind::Screen => probe_screen_permission(),
        DeviceKind::Window => probe_window_permission(),
        DeviceKind::Camera => probe_camera_permission(),
        DeviceKind::Microphone => Ok(PermissionState::Granted),
        _ => Ok(PermissionState::NotSupported),
    }
}

fn probe_screen_permission() -> Result<PermissionState, CaptureError> {
    let cfg = DesktopVideoCaptureConfig {
        source: DesktopCaptureSource::Screen {
            select: Select::Default,
        },
        time_base: Rational::new(1, 30),
        output: DesktopOutputPreference::CpuFramesOk,
        gpu_device: None,
    };
    match LinuxScreenCapture::open(&cfg) {
        Ok(mut cap) => {
            let _ = cap.close();
            Ok(PermissionState::Granted)
        }
        Err(CaptureError::AccessDenied) => Ok(PermissionState::Denied),
        Err(CaptureError::Unsupported) => Ok(PermissionState::Unknown),
        Err(e) => Err(e),
    }
}

fn probe_window_permission() -> Result<PermissionState, CaptureError> {
    // The `window` field is ignored by `LinuxWindowCapture::open` (portal
    // picker chooses interactively — see that fn's docs); any nonzero handle
    // satisfies the type.
    let window = NativeHandle::new(1).ok_or(CaptureError::InvalidInput)?;
    let cfg = DesktopVideoCaptureConfig {
        source: DesktopCaptureSource::Window { window },
        time_base: Rational::new(1, 30),
        output: DesktopOutputPreference::CpuFramesOk,
        gpu_device: None,
    };
    match LinuxWindowCapture::open(&cfg) {
        Ok(mut cap) => {
            let _ = cap.close();
            Ok(PermissionState::Granted)
        }
        Err(CaptureError::AccessDenied) => Ok(PermissionState::Denied),
        Err(CaptureError::Unsupported) => Ok(PermissionState::Unknown),
        Err(e) => Err(e),
    }
}

fn probe_camera_permission() -> Result<PermissionState, CaptureError> {
    let cfg = CameraCaptureConfig {
        select: Select::Default,
        time_base: Rational::new(1, 30),
        output: CameraOutputPreference::CpuFramesOk,
        gpu_device: None,
    };
    match LinuxCameraCapture::open(&cfg) {
        Ok(mut cap) => {
            let _ = cap.close();
            Ok(PermissionState::Granted)
        }
        Err(CaptureError::AccessDenied) => Ok(PermissionState::Denied),
        Err(CaptureError::Unsupported | CaptureError::InvalidInput) => Ok(PermissionState::Unknown),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
#[path = "capabilities_tests.rs"]
mod tests;
