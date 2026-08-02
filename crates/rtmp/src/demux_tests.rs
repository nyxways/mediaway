//! Unit tests for `Demuxer` — thin delegation to `ChunkDecoder`; the substantial chunk-level
//! behavior is covered in `chunk_encoder_tests.rs`/`chunk_decoder_tests.rs`.

#![cfg(test)]
#![allow(clippy::unwrap_used, reason = "unit tests")]

use super::Demuxer;
use crate::chunk_encoder::ChunkEncoder;
use crate::error::Error;

#[test]
fn empty_demuxer_needs_more_bytes() {
    let mut demux = Demuxer::new();
    assert!(demux.poll_message().unwrap().is_none());
}

#[test]
fn push_bytes_then_poll_message_decodes_one_message() {
    let mut enc = ChunkEncoder::new(128);
    let mut out = Vec::new();
    enc.encode_message(4, 9, 7, 1, b"abc", &mut out);

    let mut demux = Demuxer::new();
    demux.push_bytes(&out);
    let (message_type_id, timestamp_ms, payload) = demux.poll_message().unwrap().unwrap();
    assert_eq!(message_type_id, 9);
    assert_eq!(timestamp_ms, 7);
    assert_eq!(payload, b"abc");
}

#[test]
fn malformed_input_surfaces_as_error_not_panic() {
    let mut demux = Demuxer::new();
    demux.push_bytes(&[(3 << 6) | 3]); // fmt=3 with no prior header for csid 3
    let err = demux.poll_message().unwrap_err();
    assert!(matches!(err, Error::NoCachedHeader(3)));
}
