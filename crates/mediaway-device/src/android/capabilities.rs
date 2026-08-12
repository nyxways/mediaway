//! Capability / permission probing for the Android backend. See
//! [`mediaway-device` ADR-0003](../../mediaway-device/adr/0003-capability-and-permission-probe.md).
//!
//! **Zero runtime verification**: like the rest of `android/`, neither [`support`] nor
//! [`request_permission`]'s real-probe branches has been exercised against a real device —
//! compile-checked only (and not even that, without an NDK toolchain in this dev environment).

use crate::android::camera::AndroidCameraCapture;
use crate::camera::{CameraCapture, CameraCaptureConfig, CaptureOutputPreference};
use crate::{CaptureError, DeviceKind, PermissionState, Select, Support, Unavailable};
use mediaway_common::Rational;

/// Live support probe for `kind` on this device.
///
/// [`DeviceKind::Camera`] enumerates real Camera2 NDK camera IDs (`ACameraManager_getCameraIdList`,
/// no device opened). [`DeviceKind::Microphone`] has no cheap AAudio-level "is a mic present"
/// query (unlike PipeWire's daemon-connect probe on Linux) — every real Android device ships at
/// least one microphone, so this reports [`Support::Supported`] unconditionally rather than
/// pretending an unavailable probe exists. [`DeviceKind::Screen`]/[`DeviceKind::Window`]/
/// [`DeviceKind::Loopback`]/[`DeviceKind::ProcessLoopback`] have no backend reachable from this
/// parameterless probe — `MediaProjection` availability can only be determined by the host app's
/// own JNI-attached consent flow (see `screencast.rs` module docs), which this function has no
/// `JavaVM`/`Env` to attempt; `Window`/`Loopback`/`ProcessLoopback` have no Android backend at
/// all this session.
#[must_use]
pub fn support(kind: DeviceKind) -> Support {
    match kind {
        DeviceKind::Camera => camera_support(),
        DeviceKind::Microphone => Support::Supported,
        // Covers Screen/Window/Loopback/ProcessLoopback and any future `DeviceKind` variant
        // (`#[non_exhaustive]`) — see doc comment above for why Screen specifically has no
        // backend reachable from this parameterless probe, not just "not implemented".
        _ => Support::Unavailable(Unavailable::NotImplemented),
    }
}

fn camera_support() -> Support {
    if camera_id_count() > 0 {
        Support::Supported
    } else {
        Support::Unavailable(Unavailable::NoDeviceFound)
    }
}

/// Real `ACameraManager_getCameraIdList` count, no device opened — mirrors
/// `linux::camera::enumerate_camera_paths`'s "cheaper than a real open" cost class.
fn camera_id_count() -> usize {
    crate::android::camera::camera_id_count()
}

/// Best-effort permission probe for `kind`.
///
/// # Cost
///
/// [`DeviceKind::Camera`] opens a **real Camera2 NDK session** (the same path
/// [`AndroidCameraCapture::open`] takes) purely to observe whether
/// `ACAMERA_ERROR_PERMISSION_DENIED` comes back, then closes it — Camera2 NDK has no
/// separate cheap consent-probe API; opening is the only way to observe the CAMERA runtime
/// permission. Callers must not call this per frame; cache the result, and call [`support`]
/// first.
///
/// [`DeviceKind::Microphone`] returns [`PermissionState::Unknown`] rather than attempting a
/// real open: unlike Camera2's distinct `ACAMERA_ERROR_PERMISSION_DENIED` status, AAudio has no
/// documented, reliable way to distinguish a RECORD_AUDIO permission denial from any other
/// `open_stream` failure (see `mic.rs`'s `open_stream`, which maps all such failures to the
/// same [`CaptureError::Backend`]) — reporting [`PermissionState::Denied`] or
/// [`PermissionState::Granted`] here would claim a distinction this backend cannot actually
/// make. This is the documented purpose of [`PermissionState::Unknown`]: "no cheap probe
/// exists for this kind/platform; the caller must attempt to open a real session and handle
/// [`CaptureError::AccessDenied`]" — except this backend cannot even map that case to
/// `AccessDenied` today, so callers must treat any `mic.rs` open failure as ambiguous.
///
/// [`DeviceKind::Screen`] (and every other kind [`support`] reports
/// [`Support::Unavailable`] for) returns [`PermissionState::NotSupported`] — same reasoning as
/// [`support`]'s own doc comment.
///
/// # Errors
///
/// Returns the underlying [`CaptureError`] when a probe itself fails for a reason other than
/// access denial.
pub fn request_permission(kind: DeviceKind) -> Result<PermissionState, CaptureError> {
    if matches!(support(kind), Support::Unavailable(_)) {
        return Ok(PermissionState::NotSupported);
    }
    match kind {
        DeviceKind::Camera => probe_camera_permission(),
        DeviceKind::Microphone => Ok(PermissionState::Unknown),
        _ => Ok(PermissionState::NotSupported),
    }
}

fn probe_camera_permission() -> Result<PermissionState, CaptureError> {
    let cfg = CameraCaptureConfig {
        select: Select::Default,
        time_base: Rational::new(1, 30),
        output: CaptureOutputPreference::CpuFramesOk,
        gpu_device: None,
    };
    match AndroidCameraCapture::open(&cfg) {
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
