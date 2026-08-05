//! RTP fixed header (RFC 3550 §5.1) — the minimal 12-byte case only: no CSRC
//! list, no header extension, no padding. This mirrors this workspace's
//! established "narrowest self-consistent" scope pattern for other bitstream
//! headers (see crate root docs and `adr/0001-rtp-freestanding-core.md`).

#![forbid(unsafe_code)]

use bytes::Bytes;

use crate::error::Error;

/// RTP version this crate implements (RFC 3550 mandates 2 for current RTP).
pub const RTP_VERSION: u8 = 2;

/// Length in bytes of the fixed RTP header this crate builds/parses (no CSRC,
/// no extension, no padding).
pub const HEADER_LEN: usize = 12;

/// RTP fixed header fields (RFC 3550 §5.1).
///
/// `padding`/`extension` (the `P`/`X` bits) and the CSRC list are not
/// represented: [`RtpHeader::write`] always emits `P=0`, `X=0`, `CC=0`, and
/// [`RtpHeader::parse`] returns [`Error::PaddingUnsupported`] /
/// [`Error::HeaderExtensionUnsupported`] rather than silently mis-parsing a
/// packet that actually carries either. A non-zero CSRC count (`CC`) is
/// skipped over correctly (its entries are not exposed) since a lone sender —
/// this crate has no mixer/translator role — never has CSRCs to report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtpHeader {
    /// Marker bit (`M`) — profile-defined. RFC 6184 §5.3 and RFC 7798 §4.4 both
    /// set it on the last packet of an access unit (end-of-frame signal).
    pub marker: bool,
    /// Payload type (`PT`, 7 bits) — a dynamic PT value negotiated out-of-band
    /// (e.g. via SDP); this crate does not assign or validate one against a
    /// profile, only that it fits 7 bits.
    pub payload_type: u8,
    /// Sequence number — increments by one per RTP packet sent. Sans-io: this
    /// crate has no session state, so `packetize` callers get it from an
    /// internal per-`Packetizer` counter (see `h264`/`hevc` modules), not from
    /// this type directly.
    pub sequence_number: u16,
    /// RTP timestamp, scaled to the [`crate::RTP_VIDEO_CLOCK_RATE_HZ`] (90 kHz)
    /// clock mandated for H.264/HEVC video by RFC 6184 §4.2 / RFC 7798 §4.4.
    pub timestamp: u32,
    /// Synchronization source identifier — caller-owned (sans-io: no session
    /// state here to generate or collision-detect one).
    pub ssrc: u32,
}

impl RtpHeader {
    /// Append the 12-byte fixed header to `out` (`P=0`, `X=0`, `CC=0`).
    ///
    /// # Errors
    ///
    /// [`Error::PayloadTypeOutOfRange`] if `payload_type` does not fit 7 bits.
    pub fn write(&self, out: &mut Vec<u8>) -> Result<(), Error> {
        if self.payload_type & 0x80 != 0 {
            return Err(Error::PayloadTypeOutOfRange(self.payload_type));
        }
        out.push(RTP_VERSION << 6); // V=2, P=0, X=0, CC=0
        out.push((u8::from(self.marker) << 7) | self.payload_type);
        out.extend_from_slice(&self.sequence_number.to_be_bytes());
        out.extend_from_slice(&self.timestamp.to_be_bytes());
        out.extend_from_slice(&self.ssrc.to_be_bytes());
        Ok(())
    }

    /// Parse the fixed header from the start of `data`.
    ///
    /// Returns the header and the number of bytes it (plus any CSRC list)
    /// occupied — callers slice `&data[consumed..]` for the payload.
    ///
    /// # Errors
    ///
    /// - [`Error::BufferTooShort`] if `data` is shorter than the fixed header
    ///   (plus its CSRC list, if `CC > 0`).
    /// - [`Error::UnsupportedRtpVersion`] if `V != 2`.
    /// - [`Error::HeaderExtensionUnsupported`] / [`Error::PaddingUnsupported`]
    ///   if `X`/`P` are set — see the type-level docs for why this crate
    ///   refuses rather than mis-parses those packets.
    pub fn parse(data: &[u8]) -> Result<(Self, usize), Error> {
        if data.len() < HEADER_LEN {
            return Err(Error::BufferTooShort {
                needed: HEADER_LEN,
                got: data.len(),
            });
        }
        let b0 = data[0];
        let version = b0 >> 6;
        if version != RTP_VERSION {
            return Err(Error::UnsupportedRtpVersion(version));
        }
        if b0 & 0x20 != 0 {
            return Err(Error::PaddingUnsupported);
        }
        if b0 & 0x10 != 0 {
            return Err(Error::HeaderExtensionUnsupported);
        }
        let csrc_count = usize::from(b0 & 0x0F);
        let b1 = data[1];
        let marker = b1 & 0x80 != 0;
        let payload_type = b1 & 0x7F;
        let sequence_number = u16::from_be_bytes([data[2], data[3]]);
        let timestamp = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let ssrc = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);

        let consumed = HEADER_LEN + csrc_count * 4;
        if data.len() < consumed {
            return Err(Error::BufferTooShort {
                needed: consumed,
                got: data.len(),
            });
        }

        Ok((
            Self {
                marker,
                payload_type,
                sequence_number,
                timestamp,
                ssrc,
            },
            consumed,
        ))
    }
}

/// One RTP packet: fixed header + payload bytes.
///
/// [`h264::Packetizer`](crate::h264::Packetizer) / [`hevc::Packetizer`](crate::hevc::Packetizer)
/// return `Vec<RtpPacket>` (one NAL unit may become several packets under
/// FU-A/FU fragmentation); [`RtpPacket::write`] serializes one packet to bytes
/// for the caller's own socket send, and [`RtpPacket::parse`] is the inverse
/// for a caller's own socket receive. Neither this type nor this crate opens a
/// socket (sans-io).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtpPacket {
    /// Fixed RTP header for this packet.
    pub header: RtpHeader,
    /// Payload bytes (H.264/HEVC single-NAL, FU-A/FU indicator+header+fragment, …).
    pub payload: Bytes,
}

impl RtpPacket {
    /// Serialize `header` followed by `payload` to `out`.
    ///
    /// # Errors
    ///
    /// [`Error::PayloadTypeOutOfRange`] — see [`RtpHeader::write`].
    pub fn write(&self, out: &mut Vec<u8>) -> Result<(), Error> {
        self.header.write(out)?;
        out.extend_from_slice(&self.payload);
        Ok(())
    }

    /// Parse one RTP packet (header + remaining bytes as payload) from `data`.
    ///
    /// # Errors
    ///
    /// Propagates [`RtpHeader::parse`]'s errors.
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let (header, consumed) = RtpHeader::parse(data)?;
        // clone: `data` is a caller-owned receive buffer the caller will reuse
        // (e.g. an inbound UDP recv buffer) — the returned packet must own its
        // payload independently.
        let payload = Bytes::copy_from_slice(&data[consumed..]);
        Ok(Self { header, payload })
    }
}

#[cfg(test)]
#[path = "header_tests.rs"]
mod tests;
