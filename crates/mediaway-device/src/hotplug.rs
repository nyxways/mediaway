//! Device hotplug vocabulary ([`DeviceEvent`], [`DeviceHotplug`]).
//!
//! **Vocabulary only in this pass — no backend implements [`DeviceHotplug`]
//! yet.** See [ADR-0005](../adr/0005-device-selection.md) § Hotplug: v1
//! scope is Microphone/Loopback only, via `IMMNotificationClient`, and is a
//! separate follow-up task from the one that added these types. Camera and
//! Screen hotplug are deferred further still (different OS mechanism —
//! `WM_DEVICECHANGE`/`WM_DISPLAYCHANGE`, message-pump-based).

#![forbid(unsafe_code)]

use crate::capability::DeviceKind;
use crate::device_id::DeviceId;
use crate::error::CaptureError;

/// A device-change notification.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DeviceEvent {
    /// A device of `kind` with identity `id` became available.
    Added {
        /// The newly available device.
        id: DeviceId,
        /// Its kind.
        kind: DeviceKind,
    },
    /// A device of `kind` with identity `id` was removed.
    Removed {
        /// The removed device.
        id: DeviceId,
        /// Its kind.
        kind: DeviceKind,
    },
    /// The OS default device for `kind` changed.
    DefaultChanged {
        /// The kind whose default changed.
        kind: DeviceKind,
        /// The new default, or `None` if `kind` no longer has a default.
        id: Option<DeviceId>,
    },
    /// A device's state changed (e.g. enabled/disabled) without being
    /// added or removed.
    StateChanged {
        /// The device whose state changed.
        id: DeviceId,
        /// Its kind.
        kind: DeviceKind,
    },
}

/// Sync-poll device-change notifications.
///
/// Mirrors [`crate::AudioCapture::poll_frame`]'s idle convention (`Ok(None)`
/// = nothing pending) per `docs/spec/async-and-streaming.md`'s sync/poll
/// policy for platform sessions.
///
/// A concrete backend type (e.g. a future `WindowsDeviceHotplug::open(kinds:
/// &[DeviceKind]) -> Result<Self, CaptureError>`, `Type::open` shape per
/// `docs/conventions/code-style.md` § Public Rust API shape) is expected to
/// own the real OS registration and unregister it on
/// [`close`](Self::close)/`Drop` — a genuine RAII resource, since a
/// registered OS callback that is never unregistered is a real
/// leak/dangling-callback risk. **No such backend type exists yet** — see the
/// module docs.
pub trait DeviceHotplug {
    /// Pull the next pending device-change event, if any. `Ok(None)` = no
    /// event pending right now.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError`] on backend failure.
    fn poll_event(&mut self) -> Result<Option<DeviceEvent>, CaptureError>;

    /// Unregister the OS callback and free resources.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError`] on backend failure.
    fn close(&mut self) -> Result<(), CaptureError>;
}
