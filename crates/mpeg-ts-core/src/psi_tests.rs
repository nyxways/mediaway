//! Unit tests for PAT/PMT section build/parse.

#![cfg(test)]
#![allow(clippy::unwrap_used, reason = "unit tests")]

use super::{build_pat_section, build_pmt_section, parse_pat_section, parse_pmt_section};
use crate::error::Error;
use crate::types::{ElementaryStream, StreamType};

fn with_pointer(section: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8]; // pointer_field = 0
    out.extend_from_slice(section);
    out
}

#[test]
fn pat_roundtrips_pmt_pid() {
    let section = build_pat_section(1, 1, 256);
    let parsed = parse_pat_section(&with_pointer(&section)).unwrap();
    assert_eq!(parsed.pmt_pid, 256);
}

#[test]
fn pmt_roundtrips_streams() {
    let streams = [
        ElementaryStream {
            pid: 257,
            stream_type: StreamType::H264,
        },
        ElementaryStream {
            pid: 258,
            stream_type: StreamType::Aac,
        },
    ];
    let section = build_pmt_section(1, &streams);
    let parsed = parse_pmt_section(&with_pointer(&section)).unwrap();
    assert_eq!(&parsed.streams[..], &streams);
}

#[test]
fn pat_rejects_corrupted_crc() {
    let mut section = build_pat_section(1, 1, 256);
    let last = section.len() - 1;
    section[last] ^= 0xFF;
    let err = parse_pat_section(&with_pointer(&section)).unwrap_err();
    assert!(matches!(err, Error::CrcMismatch { .. }));
}

#[test]
fn pmt_rejects_wrong_table_id() {
    let section = build_pat_section(1, 1, 256); // PAT's table_id, fed to the PMT parser
    let err = parse_pmt_section(&with_pointer(&section)).unwrap_err();
    assert!(matches!(err, Error::UnexpectedTableId { .. }));
}

#[test]
fn pmt_single_stream_roundtrips() {
    let streams = [ElementaryStream {
        pid: 300,
        stream_type: StreamType::Mp3,
    }];
    let section = build_pmt_section(7, &streams);
    let parsed = parse_pmt_section(&with_pointer(&section)).unwrap();
    assert_eq!(parsed.streams.len(), 1);
    assert_eq!(parsed.streams[0], streams[0]);
}
