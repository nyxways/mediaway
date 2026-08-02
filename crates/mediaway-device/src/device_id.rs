//! Opaque device identity ([`DeviceId`]) and selection strategy ([`Select`]).
//!
//! See [ADR-0005](../adr/0005-device-selection.md).

#![forbid(unsafe_code)]

use thiserror::Error;

/// Opaque, backend-tagged device identity — a stable identity a caller can
/// persist, log, or match against [`crate::DeviceInfo`] snapshots returned by
/// a backend's `enumerate`.
///
/// One type, tagged internally — not three separate per-kind types (mirrors
/// `GpuDeviceHandle`'s "tagged variants in one enum" precedent, ADR-0013).
/// `DeviceIdRepr` stays private; construction only through the `from_*`
/// associated functions below (`#[non_exhaustive]` blocks downstream-crate
/// variant construction), matching `NativeHandle::new`'s
/// constructor-over-raw-field precedent (ADR-0013). Passing a `DeviceId` of
/// the wrong kind to a config (e.g. a `Wasapi` id to `CaptureSource::Camera`)
/// is rejected at `open()` (`CaptureError::Unsupported`), the same way every
/// other source/variant mismatch already is — not a compile-time distinction.
///
/// **No `unsafe`.** `DeviceId` holds owned `String`s, not pointers — it lives
/// in this `#![forbid(unsafe_code)]` facade crate exactly like `DeviceKind`/
/// `Support` already do.
///
/// # Identity stability is not uniform across kinds
///
/// See each [`DeviceIdRepr`] variant's rustdoc: `Wasapi` and `MediaFoundation`
/// identities are durable hardware identity; `DxgiOutput` is honestly weaker
/// (session/topology-scoped) — documented on the variant, not hidden.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceId(DeviceIdRepr);

/// Backend-tagged device identity representation. Private — construct via
/// [`DeviceId`]'s `from_*` associated functions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
enum DeviceIdRepr {
    /// `IMMDevice::GetId()` — persistent WASAPI endpoint ID string
    /// (mic / render / loopback endpoints). OS-documented persistent
    /// identity, stable across unplug/replug of the same physical endpoint
    /// and across reboots.
    Wasapi(String),
    /// `MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK` — persistent
    /// Media Foundation camera symbolic link (same attribute family already
    /// queried for `MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME` in `camera.rs`).
    /// Persistent per USB port + driver instance — moving a webcam to a
    /// different physical port yields a different symbolic link (a real,
    /// known Windows quirk, not a Mediaway limitation — same caveat V4L2
    /// `/dev/v4l/by-id/` paths carry on Linux).
    MediaFoundation(String),
    /// `DXGI_OUTPUT_DESC.DeviceName` (e.g. `"\\.\DISPLAY1"`).
    ///
    /// **Session-scoped, not a persistent hardware identity** — GDI device
    /// names can be reassigned when the display topology changes (monitor
    /// unplugged/replugged, docking/undocking). Weaker than the other two
    /// variants; a stronger EDID/`SetupAPI`-backed monitor identity is
    /// deferred (see ADR-0005 § Deferred).
    DxgiOutput(String),
}

impl DeviceId {
    /// Build a [`DeviceId`] from a `WASAPI` endpoint ID (`IMMDevice::GetId()`).
    #[must_use]
    pub fn from_wasapi_endpoint_id(id: impl Into<String>) -> Self {
        Self(DeviceIdRepr::Wasapi(id.into()))
    }

    /// Build a [`DeviceId`] from a Media Foundation camera symbolic link
    /// (`MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK`).
    #[must_use]
    pub fn from_media_foundation_symbolic_link(link: impl Into<String>) -> Self {
        Self(DeviceIdRepr::MediaFoundation(link.into()))
    }

    /// Build a [`DeviceId`] from a DXGI output `DeviceName`
    /// (`DXGI_OUTPUT_DESC.DeviceName`, e.g. `"\\.\DISPLAY1"`).
    #[must_use]
    pub fn from_dxgi_output_device_name(name: impl Into<String>) -> Self {
        Self(DeviceIdRepr::DxgiOutput(name.into()))
    }

    /// The wrapped `WASAPI` endpoint ID, or `None` if this [`DeviceId`] wraps
    /// a different kind.
    #[must_use]
    pub fn as_wasapi_endpoint_id(&self) -> Option<&str> {
        match &self.0 {
            DeviceIdRepr::Wasapi(id) => Some(id),
            DeviceIdRepr::MediaFoundation(_) | DeviceIdRepr::DxgiOutput(_) => None,
        }
    }

    /// The wrapped Media Foundation symbolic link, or `None` if this
    /// [`DeviceId`] wraps a different kind.
    #[must_use]
    pub fn as_media_foundation_symbolic_link(&self) -> Option<&str> {
        match &self.0 {
            DeviceIdRepr::MediaFoundation(link) => Some(link),
            DeviceIdRepr::Wasapi(_) | DeviceIdRepr::DxgiOutput(_) => None,
        }
    }

    /// The wrapped DXGI output `DeviceName`, or `None` if this [`DeviceId`]
    /// wraps a different kind.
    #[must_use]
    pub fn as_dxgi_output_device_name(&self) -> Option<&str> {
        match &self.0 {
            DeviceIdRepr::DxgiOutput(name) => Some(name),
            DeviceIdRepr::Wasapi(_) | DeviceIdRepr::MediaFoundation(_) => None,
        }
    }
}

impl std::fmt::Display for DeviceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            DeviceIdRepr::Wasapi(id) => write!(f, "wasapi:{id}"),
            DeviceIdRepr::MediaFoundation(link) => write!(f, "mf-symlink:{link}"),
            DeviceIdRepr::DxgiOutput(name) => write!(f, "dxgi-output:{name}"),
        }
    }
}

/// [`DeviceId`]'s [`FromStr`](std::str::FromStr) input did not start with a
/// recognized `wasapi:` / `mf-symlink:` / `dxgi-output:` tag prefix.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unrecognized device id tag prefix: {0:?}")]
pub struct ParseDeviceIdError(String);

impl std::str::FromStr for DeviceId {
    type Err = ParseDeviceIdError;

    #[allow(
        clippy::option_if_let_else,
        reason = "a chained if-let over 3 mutually exclusive tag prefixes reads clearer than nested map_or_else closures"
    )]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(rest) = s.strip_prefix("wasapi:") {
            Ok(Self::from_wasapi_endpoint_id(rest))
        } else if let Some(rest) = s.strip_prefix("mf-symlink:") {
            Ok(Self::from_media_foundation_symbolic_link(rest))
        } else if let Some(rest) = s.strip_prefix("dxgi-output:") {
            Ok(Self::from_dxgi_output_device_name(rest))
        } else {
            Err(ParseDeviceIdError(s.to_owned()))
        }
    }
}

/// Device selection strategy — replaces the raw index/ordinal fields
/// (`device_index`/`device`/`output_index`) that every capture/playback
/// config used before ADR-0005.
///
/// Owned (not `&DeviceId`/`&str`): every existing backend (`wasapi.rs`,
/// `wasapi_playback.rs`, `camera.rs`) opens its session via
/// `thread::Builder::spawn(move || ..)`, moving config-derived values into a
/// `'static` worker thread — a borrowed `Select<'a>` could not cross that
/// boundary without a lifetime parameter on every config struct in this
/// crate, a far larger ergonomics regression than the clone it would avoid.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Select {
    /// OS default for this kind (existing behavior — index/ordinal `0` today).
    #[default]
    Default,
    /// A specific device by stable [`DeviceId`] (from a backend's
    /// `enumerate`, or persisted/restored).
    Id(DeviceId),
    /// First device (per `enumerate`'s returned order) whose name contains
    /// `needle`, case-insensitively. Backend-defined enumeration order — not
    /// a promised stable global sort (same honesty already applied to
    /// `camera.rs`'s ordinal note).
    NameContains(String),
}

#[cfg(test)]
#[path = "device_id_tests.rs"]
mod tests;
