//! Encoder capability probe.
//!
//! Mirrors `mediaway-device`'s
//! [ADR-0003](../../mediaway-device/adr/0003-capability-and-permission-probe.md): ask
//! "would this backend work" without the app having to interpret an [`crate::EncodeError`]
//! from a real `AutoVideoEncoder` (`mediaway-encoder-windows/src/auto.rs`) open.
//! Platform crates implement `support(codec) -> Vec<EncoderCapability>`
//! (`mediaway_encoder_windows::auto::support` today); `mediaway::platform`
//! dispatches it the same way it dispatches `AutoEncoder::open`.

#![forbid(unsafe_code)]

use crate::auto::{Backend, EncodePathClass};

/// Why a [`Backend`] is not usable for a codec on this machine right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EncodeUnavailable {
    /// No platform code path exists for this backend/codec combination at all
    /// (e.g. AMF: no license-clear Rust binding builds on this workspace's MSRV yet —
    /// see `mediaway-encoder-amf` adr/0001).
    NotImplemented,
    /// A backend has real code, but the driver/device it needs did not answer on this
    /// machine right now.
    NoDevice,
}

/// Live availability of a [`Backend`] for a codec, measured on this machine right now —
/// not just "was this compiled in".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EncodeSupport {
    /// Usable, and the cheapest [`EncodePathClass`] the probe reached.
    Supported(EncodePathClass),
    /// Not usable right now — see [`EncodeUnavailable`] for why.
    Unavailable(EncodeUnavailable),
}

/// One row of "what encoding this codec could look like on this machine right now" —
/// the data behind a settings dropdown entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct EncoderCapability {
    /// Which backend this row describes.
    pub backend: Backend,
    /// Whether it's usable right now, and at what cost if so.
    pub support: EncodeSupport,
}

impl EncoderCapability {
    /// Build a row — a plain constructor since the struct is `#[non_exhaustive]`.
    #[must_use]
    pub const fn new(backend: Backend, support: EncodeSupport) -> Self {
        Self { backend, support }
    }
}
