//! HEVC RTP payloadization (RFC 7798).
//!
//! Implements two of RFC 7798's four payload structures: **single NAL unit
//! packet** (§4.4.1 — a NAL unit that fits under the caller's MTU budget
//! as-is, its 2-byte NAL unit header co-serving as the payload header) and
//! **FU** fragmentation (§4.4.3 — a NAL unit split across multiple RTP
//! packets). Aggregation packets (AP, §4.4.2) and PACI (§4.4.4) are a named
//! scope cut — see `adr/0001-rtp-freestanding-core.md`. The DONL field (used
//! only when `sprop-max-don-diff > 0`, i.e. decoding-order-number signaling
//! for out-of-order transmission) is never written or expected, matching this
//! crate's in-order-only depacketize scope.
//!
//! HEVC's NAL unit header is 2 bytes (RFC 7798 §1.1.4: `F`(1) `Type`(6)
//! `LayerId`(6) `TID`(3)) — unlike H.264's 1-byte header, which is why this
//! module does not share code with [`crate::h264`] despite the structural
//! similarity of single-NAL/FU packetize-depacketize.

#![forbid(unsafe_code)]

use bytes::Bytes;

use crate::error::Error;
use crate::header::{RtpHeader, RtpPacket};

/// Length in bytes of the HEVC NAL unit header / RTP payload header (RFC 7798 §1.1.4).
const NAL_HEADER_LEN: usize = 2;
/// Payload header `Type` value identifying an Aggregation Packet (RFC 7798 §4.4.2).
const AP_TYPE: u8 = 48;
/// Payload header `Type` value identifying a Fragmentation Unit (RFC 7798 §4.4.3).
const FU_TYPE: u8 = 49;
/// Payload header `Type` value identifying a PACI packet (RFC 7798 §4.4.4).
const PACI_TYPE: u8 = 50;
/// Bytes of fixed overhead an FU packet adds ahead of fragment data: 2-byte
/// payload header + 1-byte FU header (RFC 7798 Figure 9; no DONL, see module docs).
const FU_OVERHEAD: usize = NAL_HEADER_LEN + 1;

/// Decodes a 2-byte HEVC NAL unit header into `(F, Type, LayerId, TID)`.
const fn decode_nal_header(b0: u8, b1: u8) -> (u8, u8, u8, u8) {
    let f = (b0 >> 7) & 0x01;
    let nal_type = (b0 >> 1) & 0x3F;
    let layer_id = ((b0 & 0x01) << 5) | (b1 >> 3);
    let tid = b1 & 0x07;
    (f, nal_type, layer_id, tid)
}

/// Encodes `(F, Type, LayerId, TID)` into a 2-byte HEVC NAL unit header.
const fn encode_nal_header(f: u8, nal_type: u8, layer_id: u8, tid: u8) -> [u8; 2] {
    let b0 = (f << 7) | (nal_type << 1) | (layer_id >> 5);
    let b1 = ((layer_id & 0x1F) << 3) | tid;
    [b0, b1]
}

/// Packetizes single HEVC NAL units (Annex-B start codes NOT included — pass
/// just the NAL unit bytes, 2-byte header included) into one or more
/// [`RtpPacket`]s, per RFC 7798.
///
/// Owns a monotonic sequence-number counter (sans-io: no socket/session, but a
/// caller streaming many NAL units needs consistent, incrementing sequence
/// numbers across calls — matching this crate's "internal counter the session
/// owns" option, see `adr/0001`).
#[derive(Debug, Clone)]
pub struct Packetizer {
    /// Maximum RTP **payload** size in bytes this packetizer will emit per
    /// packet — i.e. network MTU minus IP/UDP/RTP header overhead, *not* the
    /// raw link MTU. See [`crate::h264::Packetizer`]'s field docs for the same
    /// contract.
    max_payload_size: usize,
    payload_type: u8,
    ssrc: u32,
    next_sequence_number: u16,
}

impl Packetizer {
    /// Start a packetizer.
    ///
    /// # Errors
    ///
    /// - [`Error::MaxPayloadSizeTooSmall`] if `max_payload_size` cannot fit
    ///   even a one-byte FU payload (`FU_OVERHEAD` + 1).
    /// - [`Error::PayloadTypeOutOfRange`] if `payload_type` does not fit 7 bits.
    pub const fn new(
        max_payload_size: usize,
        payload_type: u8,
        ssrc: u32,
        initial_sequence_number: u16,
    ) -> Result<Self, Error> {
        if max_payload_size <= FU_OVERHEAD {
            return Err(Error::MaxPayloadSizeTooSmall(max_payload_size));
        }
        if payload_type & 0x80 != 0 {
            return Err(Error::PayloadTypeOutOfRange(payload_type));
        }
        Ok(Self {
            max_payload_size,
            payload_type,
            ssrc,
            next_sequence_number: initial_sequence_number,
        })
    }

    /// Packetize one NAL unit. `timestamp` is the 90 kHz-scaled RTP timestamp
    /// ([`crate::RTP_VIDEO_CLOCK_RATE_HZ`]) shared by every packet this call
    /// produces. `marker` is set on the *last* packet this call produces only
    /// if `true` — pass `true` when `nal` is the last NAL unit of its access
    /// unit (RFC 7798 §4.4's end-of-frame convention).
    ///
    /// # Errors
    ///
    /// - [`Error::NalUnitTooShort`] if `nal` is shorter than the 2-byte NAL
    ///   unit header.
    /// - [`Error::ReservedNalUnitType`] if `nal`'s type field is 48, 49, or 50
    ///   — this crate builds AP/FU/PACI framing itself and never accepts it as
    ///   packetize input.
    pub fn packetize(
        &mut self,
        nal: &[u8],
        timestamp: u32,
        marker: bool,
    ) -> Result<Vec<RtpPacket>, Error> {
        if nal.len() < NAL_HEADER_LEN {
            return Err(Error::NalUnitTooShort {
                needed: NAL_HEADER_LEN,
                got: nal.len(),
            });
        }
        let (f, nal_type, layer_id, tid) = decode_nal_header(nal[0], nal[1]);
        if matches!(nal_type, AP_TYPE | FU_TYPE | PACI_TYPE) {
            return Err(Error::ReservedNalUnitType(nal_type));
        }

        if nal.len() <= self.max_payload_size {
            // clone: `nal` is caller-owned; the returned RtpPacket must own its
            // payload independently of the caller's buffer.
            let payload = Bytes::copy_from_slice(nal);
            return Ok(vec![self.build_packet(payload, timestamp, marker)]);
        }

        let payload_header = encode_nal_header(f, FU_TYPE, layer_id, tid);
        let body = &nal[NAL_HEADER_LEN..];
        let fragment_capacity = self.max_payload_size - FU_OVERHEAD;

        let mut packets = Vec::with_capacity(body.len().div_ceil(fragment_capacity));
        let mut offset = 0;
        while offset < body.len() {
            let end = (offset + fragment_capacity).min(body.len());
            let is_first = offset == 0;
            let is_last = end == body.len();

            let mut payload = Vec::with_capacity(FU_OVERHEAD + (end - offset));
            payload.extend_from_slice(&payload_header);
            payload.push((u8::from(is_first) << 7) | (u8::from(is_last) << 6) | nal_type);
            payload.extend_from_slice(&body[offset..end]);

            packets.push(self.build_packet(Bytes::from(payload), timestamp, is_last && marker));
            offset = end;
        }
        Ok(packets)
    }

    const fn build_packet(&mut self, payload: Bytes, timestamp: u32, marker: bool) -> RtpPacket {
        let header = RtpHeader {
            marker,
            payload_type: self.payload_type,
            sequence_number: self.next_sequence_number,
            timestamp,
            ssrc: self.ssrc,
        };
        self.next_sequence_number = self.next_sequence_number.wrapping_add(1);
        RtpPacket { header, payload }
    }
}

/// Reassembles HEVC NAL units from a stream of RTP packet **payloads**.
///
/// Arrival is assumed to be in sequence-number order. Out-of-order/
/// loss-tolerant reassembly is a named scope cut — see
/// `adr/0001-rtp-freestanding-core.md`.
#[derive(Debug, Clone, Default)]
pub struct Depacketizer {
    /// Scratch buffer for the NAL unit currently being reassembled from FU
    /// fragments; `fu_nal[0..2]` holds the reconstructed 2-byte NAL header
    /// once the start fragment (`S=1`) has been seen.
    fu_nal: Vec<u8>,
    fu_in_progress: bool,
}

impl Depacketizer {
    /// New, empty depacketize session.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one arriving RTP packet's **payload** (i.e. `RtpPacket::payload`,
    /// not the whole packet).
    ///
    /// Returns `Ok(Some(nal))` when this payload completes a NAL unit (a
    /// single-NAL-unit packet, or the final FU fragment); `Ok(None)` while a
    /// fragmented NAL unit is still in progress.
    ///
    /// # Errors
    ///
    /// - [`Error::NalUnitTooShort`] if `payload` is shorter than the 2-byte
    ///   payload header.
    /// - [`Error::AggregationPacketUnsupported`] for AP (type 48).
    /// - [`Error::PaciPacketUnsupported`] for PACI (type 50).
    /// - [`Error::FuPayloadTooShort`], [`Error::MissingFuStart`],
    ///   [`Error::UnexpectedFuStart`] for malformed/out-of-order FU input.
    pub fn depacketize(&mut self, payload: &[u8]) -> Result<Option<Bytes>, Error> {
        if payload.len() < NAL_HEADER_LEN {
            return Err(Error::NalUnitTooShort {
                needed: NAL_HEADER_LEN,
                got: payload.len(),
            });
        }
        let (_, nal_type, _, _) = decode_nal_header(payload[0], payload[1]);
        match nal_type {
            AP_TYPE => Err(Error::AggregationPacketUnsupported(nal_type)),
            FU_TYPE => self.depacketize_fu(payload),
            PACI_TYPE => Err(Error::PaciPacketUnsupported(nal_type)),
            // clone: `payload` is borrowed from the caller's own packet
            // buffer; the returned NAL unit must outlive it. The payload
            // header (RFC 7798 §4.4.1) is already an exact copy of the NAL
            // unit header, so the payload is the NAL unit as-is.
            _ => Ok(Some(Bytes::copy_from_slice(payload))),
        }
    }

    fn depacketize_fu(&mut self, payload: &[u8]) -> Result<Option<Bytes>, Error> {
        if payload.len() < FU_OVERHEAD {
            return Err(Error::FuPayloadTooShort {
                needed: FU_OVERHEAD,
                got: payload.len(),
            });
        }
        let (f, _, layer_id, tid) = decode_nal_header(payload[0], payload[1]);
        let fu_header = payload[2];
        let start = fu_header & 0x80 != 0;
        let end = fu_header & 0x40 != 0;
        let fu_type = fu_header & 0x3F;
        let fragment = &payload[FU_OVERHEAD..];

        if start {
            if self.fu_in_progress {
                return Err(Error::UnexpectedFuStart);
            }
            self.fu_nal.clear();
            self.fu_nal
                .extend_from_slice(&encode_nal_header(f, fu_type, layer_id, tid));
            self.fu_in_progress = true;
        } else if !self.fu_in_progress {
            return Err(Error::MissingFuStart);
        }

        self.fu_nal.extend_from_slice(fragment);

        if end {
            self.fu_in_progress = false;
            // clone: `fu_nal` is reused as scratch across calls; the completed
            // NAL unit returned to the caller must be independently owned.
            Ok(Some(Bytes::copy_from_slice(&self.fu_nal)))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
#[path = "hevc_tests.rs"]
mod tests;
