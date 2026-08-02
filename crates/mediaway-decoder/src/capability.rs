//! Decoder capability probe.
//!
//! Mirrors `mediaway-encoder`'s [`EncoderCapability`](../../mediaway-encoder/src/capability.rs)
//! probe (itself modeled on `mediaway-device`'s
//! [ADR-0003](../../mediaway-device/adr/0003-capability-and-permission-probe.md)): ask
//! "would this codec decode work on this machine right now" without the app having to
//! interpret a [`crate::DecodeError`] from a real decoder open.
//!
//! Unlike encode, decode has exactly one implementation per platform today — no
//! competing `Backend`s to enumerate (see `mediaway-pipeline::platform::decoder_support`'s
//! doc comment) — so this reports a single [`DecodeSupport`] per codec, not a `Vec` of
//! rows.

#![forbid(unsafe_code)]

/// Why decode is not usable for a codec on this machine right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DecodeUnavailable {
    /// No platform code path exists for this codec at all.
    NotImplemented,
    /// Real code exists, but the driver/device it needs did not answer on this
    /// machine right now.
    NoDevice,
}

/// Live availability of decode for a codec, measured on this machine right now —
/// not just "was this compiled in".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DecodeSupport {
    /// Usable right now.
    Supported,
    /// Not usable right now — see [`DecodeUnavailable`] for why.
    Unavailable(DecodeUnavailable),
}
