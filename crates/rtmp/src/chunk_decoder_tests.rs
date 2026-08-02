//! Unit tests for `ChunkDecoder` error paths and incremental buffering behavior not already
//! covered by the encoder/decoder round-trip tests in `chunk_encoder_tests.rs`.

#![cfg(test)]
#![allow(clippy::unwrap_used, reason = "unit tests")]

use super::ChunkDecoder;
use crate::error::Error;

#[test]
fn empty_input_needs_more_bytes() {
    let mut dec = ChunkDecoder::new();
    assert!(dec.poll_message().unwrap().is_none());
}

#[test]
fn fmt3_without_prior_header_errors() {
    let mut dec = ChunkDecoder::new();
    // basic header: fmt=3, csid=3, no header ever established for csid 3.
    dec.push_bytes(&[(3 << 6) | 3]);
    let err = dec.poll_message().unwrap_err();
    assert!(matches!(err, Error::NoCachedHeader(3)));
}

#[test]
fn fmt1_without_prior_header_errors() {
    let mut dec = ChunkDecoder::new();
    // basic header: fmt=1, csid=3, followed by a 7-byte fmt1 message header — but no cached
    // stream ID exists yet for csid 3.
    let mut input = vec![(1 << 6) | 3];
    input.extend_from_slice(&[0, 0, 0, 0, 0, 1, 20]);
    dec.push_bytes(&input);
    let err = dec.poll_message().unwrap_err();
    assert!(matches!(err, Error::NoCachedHeader(3)));
}

#[test]
fn zero_set_chunk_size_errors() {
    use crate::chunk_encoder::ChunkEncoder;

    let mut enc = ChunkEncoder::new(128);
    let mut out = Vec::new();
    enc.encode_message(2, 1, 0, 0, &0u32.to_be_bytes(), &mut out);

    let mut dec = ChunkDecoder::new();
    dec.push_bytes(&out);
    let err = dec.poll_message().unwrap_err();
    assert!(matches!(err, Error::InvalidChunkSize(0)));
}

#[test]
fn partial_header_waits_for_more_bytes() {
    let mut dec = ChunkDecoder::new();
    // Only the basic header byte for a fmt0 chunk — the 11-byte message header hasn't
    // arrived yet.
    dec.push_bytes(&[0x03]); // fmt=0, csid=3
    assert!(dec.poll_message().unwrap().is_none());
}
