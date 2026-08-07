//! C ABI status codes (`mediaway_status_t`).

use mediaway_container::{adts, flv, mp3, mp4, ogg, ts, wav, webm};

/// C ABI status code returned by fallible `mediaway-container-ffi` functions.
///
/// `InvalidArgument`/`InvalidState` are FFI-layer inventions — the wrapped Rust API
/// represents both as compile-time impossibilities. Everything else maps onto each
/// format's own `#[non_exhaustive]` error enum (hence [`Self::UnknownError`] as a shared
/// catch-all across all of them — one status enum for every format this crate wraps, not
/// a per-format one, since they all plug into the same `mediaway_muxer_t`/
/// `mediaway_demuxer_t`-family handles). See `adr/0001-mp4-mux-demux-c-abi.md` §2 and
/// `adr/0003-multi-format-c-abi.md`.
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
    /// Any wrapped format's error type is `#[non_exhaustive]`; catch-all for a variant
    /// this enum has no dedicated slot for.
    UnknownError = 6,
    /// This call caught a Rust panic; the handle is now poisoned.
    InternalPanic = 7,
    /// A previous call already poisoned this handle; the call was refused.
    HandlePoisoned = 8,
    /// A track's codec has no encoding in the requested container format (e.g.
    /// [`webm::Error::UnsupportedCodec`]) — a real, expected rejection, not a bug.
    UnsupportedCodec = 9,
    /// `push_packet`'s `stream_id` doesn't match any registered track (e.g.
    /// [`webm::Error::UnknownStream`]).
    UnknownStream = 10,
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

impl From<webm::Error> for MediawayStatus {
    fn from(err: webm::Error) -> Self {
        match err {
            webm::Error::UnsupportedCodec(_) => Self::UnsupportedCodec,
            webm::Error::UnknownStream(_) => Self::UnknownStream,
            // Mux(ebml_webm::MuxError), plus any future non-exhaustive variant.
            _ => Self::InvalidData,
        }
    }
}

impl From<ogg::Error> for MediawayStatus {
    fn from(_err: ogg::Error) -> Self {
        // ogg_core::Error is framing-level (bad capture pattern, CRC mismatch, oversized
        // packet, ...) — none of it maps to this enum's MP4-shaped
        // InvalidTrack/InvalidPacket distinction, so every variant collapses to
        // InvalidData (malformed/unrepresentable container data), matching how mp4::Error's
        // own non-exhaustive tail already collapses to a single bucket above.
        Self::InvalidData
    }
}

impl From<adts::Error> for MediawayStatus {
    fn from(err: adts::Error) -> Self {
        match err {
            adts::Error::FrameTooLarge(_) => Self::InvalidPacket,
            // UnsupportedSampleRate/BadSync/UnsupportedSamplingFrequencyIndex, plus any
            // future non-exhaustive variant.
            _ => Self::InvalidData,
        }
    }
}

impl From<flv::Error> for MediawayStatus {
    fn from(err: flv::Error) -> Self {
        match err {
            flv::Error::UnsupportedCodec(_) => Self::UnsupportedCodec,
            flv::Error::UnregisteredStream(_) => Self::UnknownStream,
            // Tag(flv_core::Error) (bad signature, oversized tag data, a tag written
            // before the file header, ...), plus any future non-exhaustive variant.
            _ => Self::InvalidData,
        }
    }
}

impl From<mp3::Error> for MediawayStatus {
    fn from(err: mp3::Error) -> Self {
        match err {
            // UnsupportedBitrate/UnsupportedSampleRate only ever surface from
            // `mediaway_mp3_muxer_create`, which has no status side channel
            // (adr/0007-mp3-c-abi.md) — kept for completeness against the non-exhaustive
            // error enum.
            mp3::Error::FrameBodyLengthMismatch { .. } => Self::InvalidPacket,
            // BadSyncOrReservedField/UnsupportedLayer, plus any future non-exhaustive
            // variant.
            _ => Self::InvalidData,
        }
    }
}

impl From<wav::Error> for MediawayStatus {
    fn from(_err: wav::Error) -> Self {
        // riff_wave_core::Error is entirely parse-level (not a RIFF/WAVE container, missing
        // `fmt ` chunk, truncated `fmt ` chunk, unsupported wFormatTag) — none of it maps to
        // this enum's MP4-shaped InvalidTrack/InvalidPacket distinction, so every variant
        // collapses to InvalidData, matching Ogg/ADTS's non-exhaustive-tail posture.
        Self::InvalidData
    }
}

impl From<ts::Error> for MediawayStatus {
    fn from(err: ts::Error) -> Self {
        match err {
            // Muxer construction (`mediaway_ts_muxer_create`) has no status side channel
            // (adr/0006-mpeg-ts-c-abi.md) — `InvalidPid` only ever surfaces there, so this
            // arm is exercised by `From` callers other than muxer construction (none
            // today), kept for completeness against the non-exhaustive error enum.
            ts::Error::InvalidPid(_) => Self::InvalidArgument,
            ts::Error::UnknownPid(_) => Self::UnknownStream,
            // BadSyncByte/CrcMismatch/unexpected table_id/..., plus any future
            // non-exhaustive variant.
            _ => Self::InvalidData,
        }
    }
}
