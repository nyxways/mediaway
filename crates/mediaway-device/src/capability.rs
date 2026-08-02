//! Capability / permission probing — "what's supported, and is access
//! granted" without opening a full capture session. See
//! [ADR-0003](../adr/0003-capability-and-permission-probe.md).

#![forbid(unsafe_code)]

/// Coarse capture-source kind for capability/permission queries.
///
/// Coarser than [`crate::CaptureSource`]/[`crate::AudioCaptureSource`] —
/// index/handle fields don't matter for "is this kind supported at all".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DeviceKind {
    /// Display / desktop duplication.
    Screen,
    /// Single-window capture.
    Window,
    /// Camera / video capture device.
    Camera,
    /// Microphone (capture endpoint).
    Microphone,
    /// System audio / render-endpoint loopback (no OS version gate on Windows).
    Loopback,
    /// Per-process render loopback (Windows 10 2004+ only — distinct from
    /// [`DeviceKind::Loopback`] because its gate is the OS version, not device
    /// presence).
    ProcessLoopback,
}

/// Why a [`DeviceKind`] is not available right now, when it isn't.
///
/// Three genuinely different conditions a caller should react to
/// differently: `NotImplemented` won't change without a code update;
/// `OsVersionTooOld` won't change without an OS update; `NoDeviceFound` can
/// change the moment hardware is plugged in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Unavailable {
    /// No platform backend is compiled into this binary for this OS at all
    /// (host stub), or no backend exists yet on any platform (e.g. `Camera`
    /// today).
    NotImplemented,
    /// A backend exists, but the OS API/contract this kind needs isn't present
    /// on the machine actually running (checked live, not by build target —
    /// e.g. `Windows.Graphics.Capture` requires Windows 10 1803+).
    OsVersionTooOld,
    /// Backend and OS support both exist, but no matching device was found on
    /// this machine right now (e.g. zero active microphone endpoints
    /// enumerated). This can change at runtime (device plugged in later); the
    /// other two variants cannot without a rebuild / OS update.
    NoDeviceFound,
}

/// Whether a [`DeviceKind`] is usable right now on this machine — a live
/// probe, not just "was a backend compiled for this OS" (see [`Unavailable`]
/// for the three distinct reasons it might not be).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Support {
    /// Backend, OS, and at least one device are all present.
    Supported,
    /// Not available right now — see [`Unavailable`] for why.
    Unavailable(Unavailable),
}

/// OS-level consent state for a [`DeviceKind`] that may require permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PermissionState {
    /// Access is available — already granted, or this platform requires no
    /// explicit consent for this kind (e.g. `WASAPI` render-endpoint loopback).
    Granted,
    /// The OS or portal denied access.
    Denied,
    /// No cheap probe exists for this kind/platform; the caller must attempt to
    /// open a real session and handle [`crate::CaptureError::AccessDenied`].
    Unknown,
    /// This [`DeviceKind`] has no backend here — same condition as
    /// [`Unavailable::NotImplemented`].
    NotSupported,
}
