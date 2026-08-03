//! PAT (`table_id` 0x00) and PMT (`table_id` 0x02) PSI section build/parse.
//!
//! Single-program scope only: the PAT carries exactly one `program_number` →
//! PMT-PID mapping; the PMT carries that program's elementary streams. Multi-
//! program transport streams are out of v1 scope (crate-local ADR-0001).

#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "crate-private helpers used by mux.rs/demux.rs; module itself is private"
)]

use smallvec::SmallVec;

use crate::crc::crc32_mpeg2;
use crate::error::Error;
use crate::types::{ElementaryStream, StreamType};

const PAT_TABLE_ID: u8 = 0x00;
const PMT_TABLE_ID: u8 = 0x02;

/// Build a complete PAT section (`table_id` through CRC-32).
pub(crate) fn build_pat_section(
    transport_stream_id: u16,
    program_number: u16,
    pmt_pid: u16,
) -> Vec<u8> {
    let mut body = Vec::new(); // everything after the length field, before CRC
    body.extend_from_slice(&transport_stream_id.to_be_bytes());
    body.push(0xC1); // reserved(2)='11' + version(5)=0 + current_next_indicator(1)=1
    body.push(0); // section_number
    body.push(0); // last_section_number
    body.extend_from_slice(&program_number.to_be_bytes());
    body.extend_from_slice(&(0xE000 | (pmt_pid & 0x1FFF)).to_be_bytes());

    finish_section(PAT_TABLE_ID, &body)
}

/// Build a complete PMT section (`table_id` through CRC-32). No PCR is
/// inserted anywhere in this crate (deferred — see ADR-0001), so `PCR_PID` is
/// always the reserved "unassigned" value `0x1FFF`.
pub(crate) fn build_pmt_section(program_number: u16, streams: &[ElementaryStream]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&program_number.to_be_bytes());
    body.push(0xC1); // reserved + version(0) + current_next_indicator(1)
    body.push(0); // section_number
    body.push(0); // last_section_number
    body.extend_from_slice(&(0xE000 | 0x1FFF_u16).to_be_bytes()); // PCR_PID: none assigned
    body.extend_from_slice(&0xF000u16.to_be_bytes()); // program_info_length = 0

    for stream in streams {
        body.push(stream.stream_type.value());
        body.extend_from_slice(&(0xE000 | (stream.pid & 0x1FFF)).to_be_bytes());
        body.extend_from_slice(&0xF000u16.to_be_bytes()); // ES_info_length = 0
    }

    finish_section(PMT_TABLE_ID, &body)
}

fn finish_section(table_id: u8, body: &[u8]) -> Vec<u8> {
    let section_length = body.len() + 4; // + CRC
    let mut section = Vec::with_capacity(3 + body.len() + 4);
    section.push(table_id);
    section.push(0xB0 | u8::try_from(section_length >> 8).unwrap_or(0));
    section.push(u8::try_from(section_length & 0xFF).unwrap_or(0));
    section.extend_from_slice(body);
    let crc = crc32_mpeg2(&section);
    section.extend_from_slice(&crc.to_be_bytes());
    section
}

/// Parsed PAT: this workspace only ever builds/expects a single program entry.
#[derive(Debug)]
pub(crate) struct ParsedPat {
    pub(crate) pmt_pid: u16,
}

/// Parse a PAT section from a PUSI packet's raw payload (`pointer_field` included).
pub(crate) fn parse_pat_section(payload_with_pointer: &[u8]) -> Result<ParsedPat, Error> {
    let section = strip_pointer_field(payload_with_pointer);
    verify_and_slice(section, PAT_TABLE_ID)?;

    // program_number(2) + reserved/version/current(1) + section_number(1) +
    // last_section_number(1) = bytes 3..8 within `section` (after the 3-byte
    // table_id+length header); first program entry starts at offset 8.
    let entry = &section[8..12];
    let pmt_pid = u16::from_be_bytes([entry[2] & 0x1F, entry[3]]);
    Ok(ParsedPat { pmt_pid })
}

/// Parsed PMT elementary streams.
#[derive(Debug)]
pub(crate) struct ParsedPmt {
    pub(crate) streams: SmallVec<[ElementaryStream; 4]>,
}

/// Parse a PMT section from a PUSI packet's raw payload (`pointer_field` included).
pub(crate) fn parse_pmt_section(payload_with_pointer: &[u8]) -> Result<ParsedPmt, Error> {
    let section = strip_pointer_field(payload_with_pointer);
    verify_and_slice(section, PMT_TABLE_ID)?;

    let program_info_length = usize::from(u16::from_be_bytes([section[10] & 0x0F, section[11]]));
    let mut pos = 12 + program_info_length;
    let mut streams = SmallVec::new();
    let section_length = usize::from(u16::from_be_bytes([section[1] & 0x0F, section[2]]));
    let end = 3 + section_length - 4; // exclude trailing CRC
    while pos + 5 <= end && pos + 5 <= section.len() {
        let stream_type_byte = section[pos];
        let pid = u16::from_be_bytes([section[pos + 1] & 0x1F, section[pos + 2]]);
        let es_info_length = usize::from(u16::from_be_bytes([
            section[pos + 3] & 0x0F,
            section[pos + 4],
        ]));
        if let Some(stream_type) = StreamType::from_value(stream_type_byte) {
            streams.push(ElementaryStream { pid, stream_type });
        } else {
            return Err(Error::UnrecognizedStreamType(stream_type_byte));
        }
        pos += 5 + es_info_length;
    }
    Ok(ParsedPmt { streams })
}

fn strip_pointer_field(payload_with_pointer: &[u8]) -> &[u8] {
    let pointer = usize::from(payload_with_pointer[0]);
    &payload_with_pointer[1 + pointer..]
}

/// Validate `table_id` + CRC-32 over `section` (`table_id` byte through the
/// program/stream entries, i.e. everything except the trailing CRC).
fn verify_and_slice(section: &[u8], expected_table_id: u8) -> Result<(), Error> {
    if section[0] != expected_table_id {
        return Err(Error::UnexpectedTableId {
            expected: expected_table_id,
            actual: section[0],
        });
    }
    let section_length = usize::from(u16::from_be_bytes([section[1] & 0x0F, section[2]]));
    let total_len = 3 + section_length;
    let crc_declared = u32::from_be_bytes(
        section[total_len - 4..total_len]
            .try_into()
            .unwrap_or_default(),
    );
    let computed = crc32_mpeg2(&section[..total_len - 4]);
    if computed != crc_declared {
        return Err(Error::CrcMismatch {
            expected: crc_declared,
            computed,
        });
    }
    Ok(())
}

#[cfg(test)]
#[path = "psi_tests.rs"]
mod tests;
