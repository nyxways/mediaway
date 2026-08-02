//! 188-byte TS packet read/write — shared by both PSI (`psi.rs`) and PES
//! (used by `mux.rs`/`demux.rs`) payload chunking.

#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "crate-private helpers used by psi.rs/mux.rs/demux.rs; module itself is private"
)]

use crate::error::Error;

pub(crate) const PACKET_LEN: usize = 188;
pub(crate) const SYNC_BYTE: u8 = 0x47;
const PAYLOAD_CAPACITY: usize = PACKET_LEN - 4; // 184 bytes when no adaptation field

/// Split `payload` across as many 188-byte TS packets as needed for `pid`,
/// advancing `cc` (continuity counter, wraps mod 16) once per packet. Sets
/// `payload_unit_start_indicator` only on the first packet. If `random_access`
/// is set, the first packet reserves a minimal (flags-byte-only) adaptation
/// field to carry `random_access_indicator`, shrinking that packet's payload
/// capacity by 2 bytes; any packet (first or last) that would otherwise be
/// shorter than 184 payload bytes gets adaptation-field stuffing to reach the
/// fixed 188-byte packet size — every TS packet is always exactly 188 bytes.
pub(crate) fn write_ts_packets(
    out: &mut Vec<u8>,
    pid: u16,
    cc: &mut u8,
    mut payload: &[u8],
    mut pusi: bool,
    random_access: bool,
) {
    let mut first = true;
    loop {
        let want_flags = first && random_access;
        let reserved_min = if want_flags { 2 } else { 0 };
        let available = PAYLOAD_CAPACITY - reserved_min;
        let take = payload.len().min(available);
        let chunk = &payload[..take];
        payload = &payload[take..];
        let is_final = payload.is_empty();
        let budget = if is_final {
            PAYLOAD_CAPACITY - take
        } else {
            reserved_min
        };

        out.push(SYNC_BYTE);
        let pusi_bit = u16::from(pusi) << 14;
        out.extend_from_slice(&(pusi_bit | (pid & 0x1FFF)).to_be_bytes());
        let afc: u8 = if budget > 0 { 0b11 } else { 0b01 };
        out.push((afc << 4) | (*cc & 0x0F));

        if budget > 0 {
            write_adaptation_field(out, budget, first && random_access);
        }
        out.extend_from_slice(chunk);

        *cc = (*cc + 1) & 0x0F;
        pusi = false;
        first = false;
        if is_final {
            break;
        }
    }
}

/// Write an adaptation field occupying exactly `budget` bytes on the wire
/// (including its own `adaptation_field_length` byte). `budget == 1` is the
/// special "pure stuffing" case (`adaptation_field_length = 0`, no flags byte
/// at all); `budget >= 2` always includes a mandatory flags byte (per spec,
/// whenever `adaptation_field_length > 0`), with `random_access_indicator` set
/// when requested and any leftover space filled with `0xFF` stuffing.
fn write_adaptation_field(out: &mut Vec<u8>, budget: usize, random_access: bool) {
    if budget == 1 {
        out.push(0);
        return;
    }
    let adaptation_field_length = budget - 1;
    out.push(u8::try_from(adaptation_field_length).unwrap_or(u8::MAX));
    out.push(if random_access { 0x40 } else { 0x00 });
    let stuffing = adaptation_field_length - 1;
    out.resize(out.len() + stuffing, 0xFF);
}

/// Fields extracted from one parsed TS packet.
pub(crate) struct ParsedPacket<'a> {
    pub(crate) pid: u16,
    pub(crate) pusi: bool,
    pub(crate) random_access: bool,
    pub(crate) payload: &'a [u8],
}

/// Parse one exactly-188-byte TS packet.
pub(crate) fn parse_ts_packet(packet: &[u8]) -> Result<ParsedPacket<'_>, Error> {
    debug_assert_eq!(packet.len(), PACKET_LEN);
    if packet[0] != SYNC_BYTE {
        return Err(Error::BadSyncByte);
    }
    let pusi = packet[1] & 0x40 != 0;
    let pid = (u16::from(packet[1] & 0x1F) << 8) | u16::from(packet[2]);
    let afc = (packet[3] >> 4) & 0x03;

    let mut random_access = false;
    let payload_start = if afc == 0b10 || afc == 0b11 {
        let adaptation_field_length = usize::from(packet[4]);
        if adaptation_field_length > 0 {
            random_access = packet[5] & 0x40 != 0;
        }
        5 + adaptation_field_length
    } else {
        4
    };
    let payload = if afc == 0b01 || afc == 0b11 {
        &packet[payload_start.min(PACKET_LEN)..]
    } else {
        &packet[PACKET_LEN..] // afc == 0b10 (adaptation only) or reserved: no payload
    };

    Ok(ParsedPacket {
        pid,
        pusi,
        random_access,
        payload,
    })
}

#[cfg(test)]
#[path = "packet_tests.rs"]
mod tests;
