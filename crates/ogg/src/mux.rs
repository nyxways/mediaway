//! Ogg page writer — one packet per page (simple, always spec-valid; real
//! encoders also pack multiple small packets per page, deferred — see ADR-0001).

#![forbid(unsafe_code)]

use crate::crc::crc32_ogg;
use crate::error::Error;

/// Max packet payload representable in a single page's 255-entry segment table
/// (254 full 255-byte segments + one final terminator segment of up to 254 bytes).
pub const MAX_SINGLE_PAGE_PAYLOAD: usize = 65_024;

/// Byte offset of the 4-byte CRC field within a page (after capture pattern,
/// version, flags, `granule_position`, serial, sequence).
const CRC_FIELD_OFFSET: usize = 4 + 1 + 1 + 8 + 4 + 4;

/// Writes Ogg pages for one logical bitstream (one `serial`).
///
/// Unlike a real encoder, this always emits exactly one page per
/// [`Muxer::push_packet`] call (no multi-packet-per-page batching, no
/// continuation splitting for oversized packets) — a real, bounded v1 scope
/// (crate-local ADR-0001), not a corner cut on correctness: every page this
/// writes is a fully valid, independently decodable Ogg page.
#[derive(Debug, Clone)]
pub struct Muxer {
    serial: u32,
    sequence: u32,
    bos_written: bool,
}

impl Muxer {
    /// Start a mux session for logical bitstream `serial`.
    #[must_use]
    pub const fn new(serial: u32) -> Self {
        Self {
            serial,
            sequence: 0,
            bos_written: false,
        }
    }

    /// Write one page containing exactly `packet`. The first call automatically
    /// sets the page's `bos` (beginning-of-stream) flag; pass `eos = true` on the
    /// last call for this stream.
    pub fn push_packet(
        &mut self,
        packet: &[u8],
        granule_position: i64,
        eos: bool,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        if packet.len() > MAX_SINGLE_PAGE_PAYLOAD {
            return Err(Error::PacketTooLargeForSinglePage(packet.len()));
        }
        let bos = !self.bos_written;
        self.bos_written = true;
        let segments = lacing_values_for(packet.len());

        let page_start = out.len();
        out.extend_from_slice(b"OggS");
        out.push(0); // version
        let flags = (u8::from(bos) << 1) | (u8::from(eos) << 2);
        out.push(flags);
        out.extend_from_slice(&granule_position.to_le_bytes());
        out.extend_from_slice(&self.serial.to_le_bytes());
        out.extend_from_slice(&self.sequence.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // CRC placeholder
        out.push(u8::try_from(segments.len()).unwrap_or(u8::MAX));
        out.extend_from_slice(&segments);
        out.extend_from_slice(packet);

        let crc = crc32_ogg(&out[page_start..]);
        let crc_start = page_start + CRC_FIELD_OFFSET;
        out[crc_start..crc_start + 4].copy_from_slice(&crc.to_le_bytes());

        self.sequence += 1;
        Ok(())
    }
}

fn lacing_values_for(len: usize) -> Vec<u8> {
    let mut segments = Vec::new();
    let mut remaining = len;
    while remaining >= 255 {
        segments.push(255);
        remaining -= 255;
    }
    segments.push(u8::try_from(remaining).unwrap_or(u8::MAX));
    segments
}

#[cfg(test)]
#[path = "mux_tests.rs"]
mod tests;
