//! C ABI status codes (`mediaway_status_t`).

use mediaway_container::mp4;

/// C ABI status code returned by fallible `mediaway-container-ffi` functions.
///
/// `InvalidArgument`/`InvalidState` are FFI-layer inventions — the wrapped Rust API
/// represents both as compile-time impossibilities. Everything else maps onto
/// [`mp4::Error`] (`#[non_exhaustive]`, hence [`Self::UnknownError`] as a catch-all).
/// See `adr/0001-mp4-mux-demux-c-abi.md` §2.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediawayStatus {
    /// Success.
    Ok = 0,
    /// Null pointer, out-of-range index, or mismatched pointer/length pair.
    InvalidArgument = 1,
    /// Typestate violation: `add_*_track` on a `Live` muxer, or `push_packet`/`flush`/
    /// `poll_bytes` on an `Open` muxer.
    InvalidState = 2,
    /// [`mp4::Error::InvalidTrack`].
    InvalidTrack = 3,
    /// [`mp4::Error::InvalidPacket`].
    InvalidPacket = 4,
    /// [`mp4::Error::InvalidData`].
    InvalidData = 5,
    /// `mp4::Error` is `#[non_exhaustive]`; catch-all for a future variant.
    UnknownError = 6,
    /// This call caught a Rust panic; the handle is now poisoned.
    InternalPanic = 7,
    /// A previous call already poisoned this handle; the call was refused.
    HandlePoisoned = 8,
}

impl From<mp4::Error> for MediawayStatus {
    fn from(err: mp4::Error) -> Self {
        match err {
            mp4::Error::InvalidTrack => Self::InvalidTrack,
            mp4::Error::InvalidPacket => Self::InvalidPacket,
            mp4::Error::InvalidData => Self::InvalidData,
            _ => Self::UnknownError,
        }
    }
}
