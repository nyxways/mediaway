//! Device enumeration snapshot ([`DeviceInfo`]). See
//! [ADR-0005](../adr/0005-device-selection.md).

#![forbid(unsafe_code)]

use crate::capability::DeviceKind;
use crate::device_id::DeviceId;

/// Owned, detached snapshot of one enumerated device — `Clone + Send +
/// 'static` (plain owned data, no borrows, no platform handle) by
/// construction.
///
/// **Free-function shape, not a `Devices` struct/trait** — matching
/// ADR-0003's `support`/`request_permission` precedent exactly: each
/// platform crate exposes its own `enumerate(kind: DeviceKind) ->
/// Result<Vec<DeviceInfo>, CaptureError>` free function (e.g.
/// `mediaway-device-windows::enumeration::enumerate`); this facade declares
/// only the vocabulary type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceInfo {
    /// Stable device identity — pass to [`crate::Select::Id`] to reopen this
    /// exact device.
    pub id: DeviceId,
    /// Coarse kind this device was enumerated under (reused from
    /// [`DeviceKind`], ADR-0003 — no new parallel enum).
    pub kind: DeviceKind,
    /// Human-readable name (e.g. `IPropertyStore`/`PKEY_Device_FriendlyName`,
    /// `MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME`). Not consent-gated on any
    /// backend this workspace targets today (see ADR-0005).
    pub name: String,
    /// Whether this is the current OS-level default for `kind`. Semantics
    /// are honest per kind, not uniformly guessed — e.g. Camera has no OS
    /// "default camera" concept and a backend must always report `false`
    /// for it, never guess (see ADR-0005's `is_default` table).
    pub is_default: bool,
    /// Backend-defined position in this enumeration call's result order
    /// (0-based). Not guaranteed stable across separate calls/hotplugs — a
    /// convenience for "pick the Nth", not a persistent identity (use `id`
    /// for that).
    pub ordinal: u32,
}
