//! Sans-IO RTP payloadization for H.264 and HEVC — no OS I/O, no Mediaway types.
//!
//! Builds and parses the RTP fixed header ([`RtpHeader`]/[`RtpPacket`], RFC
//! 3550 §5.1's 12-byte case) and the H.264 (RFC 6184) / HEVC (RFC 7798) RTP
//! payload formats: single-NAL-unit packets and FU-A/FU fragmentation for NAL
//! units larger than a caller-supplied per-packet payload budget. See
//! [`h264`] and [`hevc`] — they mirror each other's shape but stay separate
//! modules because H.264's 1-byte NAL header and HEVC's 2-byte NAL header (plus
//! differing FU header bit layouts) genuinely differ.
//!
//! Deliberately out of scope (see
//! `crates/rtp-core/adr/0001-rtp-freestanding-core.md`): socket I/O (caller
//! owns the transport — this crate only ever takes/returns byte buffers),
//! RTCP, SRTP, aggregation packets (STAP-A/STAP-B/MTAP for H.264; AP for
//! HEVC), interleaved-mode FU-B (H.264) and PACI (HEVC), out-of-order /
//! loss-tolerant reassembly, and any codec other than H.264/HEVC.

#![forbid(unsafe_code)]

mod error;
pub mod h264;
mod header;
pub mod hevc;

pub use error::Error;
pub use header::{HEADER_LEN, RTP_VERSION, RtpHeader, RtpPacket};

/// RTP clock rate for H.264 and HEVC video, in Hz.
///
/// RFC 6184 §4.2 and RFC 7798 §4.4 both mandate 90 kHz for video; it is not
/// configurable. Callers converting from a presentation timestamp + timebase
/// to the `timestamp: u32` this crate's `packetize` methods take should scale
/// against this constant.
pub const RTP_VIDEO_CLOCK_RATE_HZ: u32 = 90_000;
