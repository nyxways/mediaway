//! H.264 RTP payloadization (RFC 6184).
//!
//! Implements two of RFC 6184's payload structures: **single NAL unit packet**
//! (§5.6 — a NAL unit that fits under the caller's MTU budget as-is, the first
//! byte of the NAL unit co-serving as the payload) and **FU-A** fragmentation
//! (§5.8 — a NAL unit split across multiple RTP packets). Aggregation packets
//! (STAP-A/STAP-B/MTAP16/MTAP24, §5.7) and the interleaved-mode FU-B are a
//! named scope cut — see `adr/0001-rtp-freestanding-core.md`.

#![forbid(unsafe_code)]

use bytes::Bytes;

use crate::error::Error;
use crate::header::{RtpHeader, RtpPacket};

/// FU indicator `Type` value identifying an FU-A (RFC 6184 §5.8).
const FU_A_TYPE: u8 = 28;
/// FU indicator `Type` value identifying an FU-B (interleaved mode only,
/// unsupported by this crate — RFC 6184 §5.8).
const FU_B_TYPE: u8 = 29;
/// Bytes of fixed overhead an FU-A packet adds ahead of fragment data: one FU
/// indicator octet + one FU header octet (RFC 6184 Figure 14).
const FU_A_OVERHEAD: usize = 2;

/// Packetizes single H.264 NAL units (Annex-B start codes NOT included — pass
/// just the NAL unit bytes, header byte included) into one or more
/// [`RtpPacket`]s, per RFC 6184.
///
/// Owns a monotonic sequence-number counter (sans-io: no socket/session, but a
/// caller streaming many NAL units needs consistent, incrementing sequence
/// numbers across calls — matching this crate's "internal counter the session
/// owns" option, see `adr/0001`).
#[derive(Debug, Clone)]
pub struct Packetizer {
    /// Maximum RTP **payload** size in bytes this packetizer will emit per
    /// packet — i.e. network MTU minus IP/UDP/RTP header overhead, *not* the
    /// raw link MTU. This crate has no network-layer knowledge, so the caller
    /// must supply the already-reduced budget (e.g. `1500 - 20 - 8 - 12 =
    /// 1460` for standard Ethernet/IPv4 with no IP options).
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
    ///   even a one-byte FU-A payload (`FU_A_OVERHEAD` + 1).
    /// - [`Error::PayloadTypeOutOfRange`] if `payload_type` does not fit 7 bits.
    pub const fn new(
        max_payload_size: usize,
        payload_type: u8,
        ssrc: u32,
        initial_sequence_number: u16,
    ) -> Result<Self, Error> {
        if max_payload_size <= FU_A_OVERHEAD {
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
    /// produces (RFC 3550 §5.1: packets from the same access unit carry equal
    /// timestamps). `marker` is set on the *last* packet this call produces
    /// only if `true` — pass `true` when `nal` is the last NAL unit of its
    /// access unit (RFC 6184 §5.3's end-of-frame convention).
    ///
    /// # Errors
    ///
    /// - [`Error::NalUnitTooShort`] if `nal` is empty.
    /// - [`Error::ReservedNalUnitType`] if `nal`'s type field (RFC 6184 Table 1)
    ///   is 0 or in `24..=31` — this crate builds aggregation/FU/reserved
    ///   framing itself and never accepts it as packetize input.
    pub fn packetize(
        &mut self,
        nal: &[u8],
        timestamp: u32,
        marker: bool,
    ) -> Result<Vec<RtpPacket>, Error> {
        let Some(&nal_header) = nal.first() else {
            return Err(Error::NalUnitTooShort { needed: 1, got: 0 });
        };
        let nal_type = nal_header & 0x1F;
        if !(1..=23).contains(&nal_type) {
            return Err(Error::ReservedNalUnitType(nal_type));
        }

        if nal.len() <= self.max_payload_size {
            // clone: `nal` is caller-owned; the returned RtpPacket must own its
            // payload independently of the caller's buffer.
            let payload = Bytes::copy_from_slice(nal);
            return Ok(vec![self.build_packet(payload, timestamp, marker)]);
        }

        let f = nal_header & 0x80;
        let nri = nal_header & 0x60;
        let fu_indicator = f | nri | FU_A_TYPE;
        let body = &nal[1..];
        let fragment_capacity = self.max_payload_size - FU_A_OVERHEAD;

        let mut packets = Vec::with_capacity(body.len().div_ceil(fragment_capacity));
        let mut offset = 0;
        while offset < body.len() {
            let end = (offset + fragment_capacity).min(body.len());
            let is_first = offset == 0;
            let is_last = end == body.len();

            let mut payload = Vec::with_capacity(FU_A_OVERHEAD + (end - offset));
            payload.push(fu_indicator);
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

/// Reassembles H.264 NAL units from a stream of RTP packet **payloads**.
///
/// Arrival is assumed to be in sequence-number order (per RFC 6184 §5.6/§5.8's
/// own decoding order requirement). Out-of-order/loss-tolerant reassembly is a
/// named scope cut — see `adr/0001-rtp-freestanding-core.md`.
#[derive(Debug, Clone, Default)]
pub struct Depacketizer {
    /// Scratch buffer for the NAL unit currently being reassembled from FU-A
    /// fragments; `fu_nal[0]` holds the reconstructed NAL header byte once the
    /// start fragment (`S=1`) has been seen.
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
    /// single-NAL-unit packet, or the final FU-A fragment); `Ok(None)` while a
    /// fragmented NAL unit is still in progress.
    ///
    /// # Errors
    ///
    /// - [`Error::NalUnitTooShort`] if `payload` is empty.
    /// - [`Error::AggregationPacketUnsupported`] for STAP-A/STAP-B/MTAP16/MTAP24.
    /// - [`Error::InterleavedFragmentUnsupported`] for FU-B.
    /// - [`Error::UnsupportedNalUnitType`] for reserved types 0/30/31.
    /// - [`Error::FuPayloadTooShort`], [`Error::MissingFuStart`],
    ///   [`Error::UnexpectedFuStart`] for malformed/out-of-order FU-A input.
    pub fn depacketize(&mut self, payload: &[u8]) -> Result<Option<Bytes>, Error> {
        let Some(&first) = payload.first() else {
            return Err(Error::NalUnitTooShort { needed: 1, got: 0 });
        };
        let nal_type = first & 0x1F;
        match nal_type {
            1..=23 => {
                // clone: `payload` is borrowed from the caller's own packet
                // buffer; the returned NAL unit must outlive it.
                Ok(Some(Bytes::copy_from_slice(payload)))
            }
            24..=27 => Err(Error::AggregationPacketUnsupported(nal_type)),
            FU_A_TYPE => self.depacketize_fu_a(payload),
            FU_B_TYPE => Err(Error::InterleavedFragmentUnsupported(nal_type)),
            _ => Err(Error::UnsupportedNalUnitType(nal_type)),
        }
    }

    fn depacketize_fu_a(&mut self, payload: &[u8]) -> Result<Option<Bytes>, Error> {
        if payload.len() < FU_A_OVERHEAD {
            return Err(Error::FuPayloadTooShort {
                needed: FU_A_OVERHEAD,
                got: payload.len(),
            });
        }
        let fu_indicator = payload[0];
        let fu_header = payload[1];
        let start = fu_header & 0x80 != 0;
        let end = fu_header & 0x40 != 0;
        let original_type = fu_header & 0x1F;
        let fragment = &payload[FU_A_OVERHEAD..];

        if start {
            if self.fu_in_progress {
                return Err(Error::UnexpectedFuStart);
            }
            let f = fu_indicator & 0x80;
            let nri = fu_indicator & 0x60;
            self.fu_nal.clear();
            self.fu_nal.push(f | nri | original_type);
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
#[path = "h264_tests.rs"]
mod tests;
