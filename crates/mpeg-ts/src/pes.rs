//! PES (Packetized Elementary Stream) header build/parse — PTS/DTS only (no
//! ESCR/ES_rate/trick-mode/PES_CRC/extension fields; see crate-local ADR-0001).

#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "crate-private helpers used by mux.rs/demux.rs; module itself is private"
)]

use crate::error::Error;

/// Conventional MPEG-TS `stream_id` values (video: `0xE0`, audio: `0xC0`).
pub(crate) const fn stream_id_for(is_video: bool) -> u8 {
    if is_video { 0xE0 } else { 0xC0 }
}

/// Build the PES header (start code through PTS/DTS) for a payload of
/// `payload_len` bytes. `PES_packet_length` is `0` ("unbounded", valid for
/// video per spec) when the true length would overflow 16 bits.
pub(crate) fn build_pes_header(
    stream_id: u8,
    payload_len: usize,
    pts_90k: u64,
    dts_90k: Option<u64>,
) -> Vec<u8> {
    let (pts_dts_flags, header_data_length, ts_bytes, pts_prefix) = if dts_90k.is_some() {
        (0b11u8, 10u8, 10usize, 0b0011u8)
    } else {
        (0b10u8, 5u8, 5usize, 0b0010u8)
    };
    let after_length_len = 3 + ts_bytes + payload_len; // 2 flag bytes + header_data_length byte + timestamps + payload
    let pes_packet_length = u16::try_from(after_length_len).unwrap_or(0);

    let mut out = Vec::with_capacity(9 + ts_bytes);
    out.extend_from_slice(&[0x00, 0x00, 0x01]);
    out.push(stream_id);
    out.extend_from_slice(&pes_packet_length.to_be_bytes());
    out.push(0x80); // '10' + scrambling(00) + priority(0) + alignment(0) + copyright(0) + original(0)
    out.push(pts_dts_flags << 6);
    out.push(header_data_length);
    write_timestamp(pts_prefix, pts_90k, &mut out);
    if let Some(dts) = dts_90k {
        write_timestamp(0b0001, dts, &mut out);
    }
    out
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "every operand is bit-masked to fit u8 immediately before the cast"
)]
fn write_timestamp(prefix4: u8, value33: u64, out: &mut Vec<u8>) {
    let v = value33 & 0x1_FFFF_FFFF; // 33 bits
    out.push((prefix4 << 4) | ((((v >> 30) & 0x07) as u8) << 1) | 1);
    out.push(((v >> 22) & 0xFF) as u8);
    out.push((((v >> 15) & 0x7F) as u8) << 1 | 1);
    out.push(((v >> 7) & 0xFF) as u8);
    out.push((((v & 0x7F) as u8) << 1) | 1);
}

fn read_timestamp(bytes: &[u8]) -> u64 {
    let b = [bytes[0], bytes[1], bytes[2], bytes[3], bytes[4]];
    (u64::from((b[0] >> 1) & 0x07) << 30)
        | (u64::from(b[1]) << 22)
        | (u64::from((b[2] >> 1) & 0x7F) << 15)
        | (u64::from(b[3]) << 7)
        | u64::from((b[4] >> 1) & 0x7F)
}

/// Parsed PES header fields.
pub(crate) struct ParsedPesHeader {
    pub(crate) header_len: usize,
    pub(crate) pts_90k: u64,
    pub(crate) dts_90k: Option<u64>,
}

/// Parse a PES header starting at `data[0]` (the `0x00 0x00 0x01` start code).
/// Returns the header length consumed (payload starts at that offset).
pub(crate) fn parse_pes_header(data: &[u8]) -> Result<ParsedPesHeader, Error> {
    if data.len() < 9 || data[0] != 0x00 || data[1] != 0x00 || data[2] != 0x01 {
        return Err(Error::BadPesStartCode);
    }
    let pts_dts_flags = (data[7] >> 6) & 0x03;
    let header_data_length = usize::from(data[8]);
    let ts_start = 9;

    let (pts_90k, dts_90k) = match pts_dts_flags {
        0b10 => (read_timestamp(&data[ts_start..ts_start + 5]), None),
        0b11 => (
            read_timestamp(&data[ts_start..ts_start + 5]),
            Some(read_timestamp(&data[ts_start + 5..ts_start + 10])),
        ),
        _ => (0, None),
    };

    Ok(ParsedPesHeader {
        header_len: ts_start + header_data_length,
        pts_90k,
        dts_90k,
    })
}

#[cfg(test)]
#[path = "pes_tests.rs"]
mod tests;
