//! Public error type.

#![forbid(unsafe_code)]

/// Errors from RTP header build/parse and H.264/HEVC payloadization.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Fewer bytes were available than the field being parsed requires.
    #[error("buffer too short: needed at least {needed} bytes, got {got}")]
    BufferTooShort {
        /// Minimum byte count required to continue parsing.
        needed: usize,
        /// Byte count actually available.
        got: usize,
    },
    /// The parsed fixed-header `V` field was not 2 (RFC 3550 §5.1).
    #[error("unsupported RTP version {0} (this crate only implements RFC 3550 version 2)")]
    UnsupportedRtpVersion(u8),
    /// `RtpHeader::payload_type` does not fit the 7-bit `PT` field (max 127).
    #[error("payload type {0} does not fit the 7-bit PT field (max 127)")]
    PayloadTypeOutOfRange(u8),
    /// The parsed header's extension bit (`X`) is set; this crate's minimal
    /// 12-byte header (no CSRC list, no extension, no padding — see crate docs)
    /// cannot skip over a header extension block it does not parse.
    #[error(
        "RTP header extension bit (X) is set; this crate's minimal 12-byte header does not parse extensions"
    )]
    HeaderExtensionUnsupported,
    /// The parsed header's padding bit (`P`) is set; this crate does not strip
    /// trailing padding octets.
    #[error(
        "RTP padding bit (P) is set; this crate's minimal 12-byte header does not strip padding"
    )]
    PaddingUnsupported,
    /// `packetize` was called with a NAL unit shorter than its mandatory header
    /// (1 byte for H.264, 2 bytes for HEVC).
    #[error("NAL unit too short: needed at least {needed} header bytes, got {got}")]
    NalUnitTooShort {
        /// Minimum NAL unit length (header size) for this codec.
        needed: usize,
        /// Byte count actually supplied.
        got: usize,
    },
    /// `max_payload_size` cannot fit even a one-byte fragmentation-unit payload
    /// (H.264: 2 bytes of FU-A overhead; HEVC: 3 bytes of FU overhead).
    #[error("max_payload_size {0} is too small to fit any fragmentation-unit payload")]
    MaxPayloadSizeTooSmall(usize),
    /// The input NAL unit's type field is one this crate reserves for RTP
    /// payload-structure framing (aggregation / fragmentation / PACI) and
    /// therefore never accepts as *input* to `packetize` — those bytes are
    /// produced internally, not handed in by the caller.
    #[error(
        "NAL unit type {0} is reserved for RTP payload-structure framing (aggregation/fragmentation/PACI) and cannot be packetized as input"
    )]
    ReservedNalUnitType(u8),
    /// `depacketize` received an aggregation packet (H.264 STAP-A/STAP-B/MTAP16/
    /// MTAP24, type 24-27; HEVC AP, type 48) — out of scope, see crate docs.
    #[error(
        "aggregation packets (type {0}) are not supported; depacketize only handles single-NAL and fragmentation-unit packets"
    )]
    AggregationPacketUnsupported(u8),
    /// `depacketize` received an H.264 FU-B (type 29) — interleaved packetization
    /// mode is out of scope, see crate docs.
    #[error("interleaved FU-B fragments (H.264 NAL type {0}) are not supported")]
    InterleavedFragmentUnsupported(u8),
    /// `depacketize` received an HEVC PACI packet (type 50) — out of scope, see
    /// crate docs.
    #[error("PACI packets (HEVC payload type {0}) are not supported")]
    PaciPacketUnsupported(u8),
    /// A fragmentation-unit RTP payload was shorter than its own fixed overhead
    /// (indicator/header bytes), before any fragment data.
    #[error("fragmentation-unit payload too short: needed at least {needed} bytes, got {got}")]
    FuPayloadTooShort {
        /// Minimum payload length (FU indicator/header overhead) for this codec.
        needed: usize,
        /// Byte count actually available.
        got: usize,
    },
    /// A fragmentation-unit continuation (`S=0`) arrived with no start fragment
    /// (`S=1`) currently in progress — depacketize assumes in-order arrival
    /// (see crate docs); a real loss/reorder-tolerant reassembler is out of scope.
    #[error("fragmentation-unit continuation (S=0) arrived with no fragment in progress")]
    MissingFuStart,
    /// A fragmentation-unit start (`S=1`) arrived while a previous fragmented
    /// NAL unit was still in progress (its end fragment, `E=1`, was never seen).
    #[error(
        "fragmentation-unit start (S=1) arrived while a previous fragment was still in progress (missing end fragment)"
    )]
    UnexpectedFuStart,
    /// `depacketize` received a single-NAL-unit packet whose type field is
    /// reserved/unsupported (H.264: 0, 30, 31).
    #[error("reserved/unsupported NAL unit type {0} in single-NAL-unit packet")]
    UnsupportedNalUnitType(u8),
}
