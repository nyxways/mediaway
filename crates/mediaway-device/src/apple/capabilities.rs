//! Capability / permission probing for the Apple backend. See
//! [`mediaway-device` ADR-0003](../../mediaway-device/adr/0003-capability-and-permission-probe.md).
//!
//! **Zero runtime verification**: like the rest of `apple/`, none of [`support`]/
//! [`request_permission`]'s real-probe branches has been exercised against a real device —
//! compile-checked only (and not even that, without macOS/Xcode in this dev environment).

use crate::{CaptureError, DeviceKind, PermissionState, Support, Unavailable};
use objc2_av_foundation::{
    AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio, AVMediaTypeVideo,
};

/// Live support probe for `kind` on this device.
///
/// [`DeviceKind::Camera`]/[`DeviceKind::Microphone`] both report [`Support::Supported`]
/// unconditionally — `AVCaptureDevice::authorizationStatusForMediaType` reports *permission*
/// state, not hardware presence, and every real Apple device this crate targets ships at least
/// one camera and microphone; there is no cheap NDK/V4L2-style device-enumeration probe to
/// prefer instead. [`DeviceKind::Screen`] reports [`Support::Supported`] unconditionally on
/// macOS (`ScreenCaptureKit` requires no separate "is capture available" query beyond the
/// permission check itself, see [`request_permission`]) and reflects `RPScreenRecorder.isAvailable`
/// on iOS (a real, cheap, synchronous property distinct from a permission check — see module
/// docs on [`super::replaykit`]). [`DeviceKind::Window`]/[`DeviceKind::Loopback`]/
/// [`DeviceKind::ProcessLoopback`] have no Apple backend this session.
#[must_use]
pub fn support(kind: DeviceKind) -> Support {
    match kind {
        DeviceKind::Camera | DeviceKind::Microphone => Support::Supported,
        DeviceKind::Screen => screen_support(),
        _ => Support::Unavailable(Unavailable::NotImplemented),
    }
}

#[cfg(target_os = "macos")]
fn screen_support() -> Support {
    Support::Supported
}

#[cfg(target_os = "ios")]
fn screen_support() -> Support {
    // SAFETY: plain, always-safe-to-call singleton + property accessor.
    let available = unsafe { objc2_replay_kit::RPScreenRecorder::sharedRecorder().isAvailable() };
    if available {
        Support::Supported
    } else {
        Support::Unavailable(Unavailable::NoDeviceFound)
    }
}

/// Best-effort permission probe for `kind`.
///
/// # Cost
///
/// [`DeviceKind::Camera`]/[`DeviceKind::Microphone`] call the real, dedicated
/// `AVCaptureDevice::authorizationStatusForMediaType` — a cheap, synchronous query, **not** a
/// consent-prompting call (unlike `requestAccessForMediaType:completionHandler:`, which this
/// function deliberately does not call — prompting the user is a caller/host-app decision, this
/// probe only observes the current status). [`DeviceKind::Screen`] on macOS calls
/// `CGPreflightScreenCaptureAccess` (cheap, no dialog) for a `NotSupported`/`Unknown` split, but
/// **cannot** cheaply distinguish granted-vs-not-yet-decided without `CGRequestScreenCaptureAccess`
/// showing the TCC dialog — so this returns [`PermissionState::Unknown`] rather than guessing;
/// callers that want a definitive answer must call `CGRequestScreenCaptureAccess` themselves
/// (outside this crate's `capabilities` API, which never shows UI). On iOS, `ReplayKit`'s own
/// consent UI is presented automatically by `startCaptureWithHandler` itself — no separate
/// preflight-without-prompting call exists (ADR-0004 § Permission model) — this returns
/// [`PermissionState::Unknown`] unconditionally for the same "the API *is* the consent
/// mechanism" reason `linux-capture.md`'s portal-based probe documents.
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
        DeviceKind::Camera => Ok(map_authorization(unsafe {
            AVCaptureDevice::authorizationStatusForMediaType(AVMediaTypeVideo)
        })),
        DeviceKind::Microphone => Ok(map_authorization(unsafe {
            AVCaptureDevice::authorizationStatusForMediaType(AVMediaTypeAudio)
        })),
        DeviceKind::Screen => screen_permission(),
        _ => Ok(PermissionState::NotSupported),
    }
}

fn map_authorization(status: AVAuthorizationStatus) -> PermissionState {
    match status {
        AVAuthorizationStatus::Authorized => PermissionState::Granted,
        AVAuthorizationStatus::Denied | AVAuthorizationStatus::Restricted => {
            PermissionState::Denied
        }
        _ => PermissionState::Unknown,
    }
}

#[cfg(target_os = "macos")]
fn screen_permission() -> Result<PermissionState, CaptureError> {
    // SAFETY: plain, safe, no-dialog C function.
    let granted = unsafe { objc2_core_graphics::CGPreflightScreenCaptureAccess() };
    Ok(if granted {
        PermissionState::Granted
    } else {
        PermissionState::Unknown
    })
}

#[cfg(target_os = "ios")]
fn screen_permission() -> Result<PermissionState, CaptureError> {
    Ok(PermissionState::Unknown)
}

#[cfg(test)]
#[path = "capabilities_tests.rs"]
mod tests;
