//! Unit tests for `ChunkEncoder`, including encode→decode round trips against
//! `ChunkDecoder` (the two are tested together since a chunk stream only means something as
//! a matched encode/decode pair).

#![cfg(test)]
#![allow(clippy::unwrap_used, reason = "unit tests")]

use super::ChunkEncoder;
use crate::chunk_decoder::ChunkDecoder;

#[test]
fn single_small_message_round_trips() {
    let mut enc = ChunkEncoder::new(128);
    let mut out = Vec::new();
    enc.encode_message(3, 20, 1000, 0, b"hello world", &mut out);

    let mut dec = ChunkDecoder::new();
    dec.push_bytes(&out);
    let (message_type_id, timestamp_ms, payload) = dec.poll_message().unwrap().unwrap();
    assert_eq!(message_type_id, 20);
    assert_eq!(timestamp_ms, 1000);
    assert_eq!(payload, b"hello world");
    assert!(dec.poll_message().unwrap().is_none());
}

#[test]
fn message_larger_than_chunk_size_fragments_and_reassembles() {
    let mut enc = ChunkEncoder::new(16);
    let mut out = Vec::new();
    let payload: Vec<u8> = (0..100u16).map(|i| u8::try_from(i).unwrap_or(0)).collect();
    enc.encode_message(4, 9, 5000, 1, &payload, &mut out);
    // First chunk: 1-byte basic header + 11-byte fmt0 header + 16 bytes payload = 28 bytes.
    // Each continuation: 1-byte basic header (fmt=3) + 16 bytes payload = 17 bytes.
    // 100 bytes total => first 16 + 6 continuations of 14 (last one shorter).
    assert!(out.len() > payload.len()); // framing overhead present

    let mut dec = ChunkDecoder::new();
    dec.set_chunk_size(16); // matches the encoder; real streams sync this via Set Chunk Size
    dec.push_bytes(&out);
    let (message_type_id, timestamp_ms, decoded) = dec.poll_message().unwrap().unwrap();
    assert_eq!(message_type_id, 9);
    assert_eq!(timestamp_ms, 5000);
    assert_eq!(decoded, payload);
}

#[test]
fn message_fed_byte_by_byte_still_reassembles() {
    let mut enc = ChunkEncoder::new(8);
    let mut out = Vec::new();
    let payload = b"the quick brown fox jumps".to_vec();
    enc.encode_message(5, 8, 42, 1, &payload, &mut out);

    let mut dec = ChunkDecoder::new();
    dec.set_chunk_size(8); // matches the encoder; real streams sync this via Set Chunk Size
    let mut result = None;
    for byte in &out {
        dec.push_bytes(std::slice::from_ref(byte));
        if let Some(msg) = dec.poll_message().unwrap() {
            result = Some(msg);
            break;
        }
    }
    let (message_type_id, timestamp_ms, decoded) = result.unwrap();
    assert_eq!(message_type_id, 8);
    assert_eq!(timestamp_ms, 42);
    assert_eq!(decoded, payload);
}

#[test]
fn second_message_same_csid_uses_compressed_header_and_still_decodes() {
    let mut enc = ChunkEncoder::new(128);
    let mut out = Vec::new();
    enc.encode_message(6, 9, 1000, 1, b"frame one", &mut out);
    enc.encode_message(6, 9, 1033, 1, b"frame two!", &mut out); // same type+stream, new length -> fmt1

    let mut dec = ChunkDecoder::new();
    dec.push_bytes(&out);
    let (t1, ts1, p1) = dec.poll_message().unwrap().unwrap();
    let (t2, ts2, p2) = dec.poll_message().unwrap().unwrap();
    assert_eq!((t1, ts1, p1), (9, 1000, b"frame one".to_vec()));
    assert_eq!((t2, ts2, p2), (9, 1033, b"frame two!".to_vec()));
}

#[test]
fn constant_delta_messages_use_fmt3_and_still_decode() {
    let mut enc = ChunkEncoder::new(128);
    let mut out = Vec::new();
    // Same type/length/stream and constant 33ms delta on every message after the second.
    enc.encode_message(6, 9, 1000, 1, b"0123456789", &mut out);
    enc.encode_message(6, 9, 1033, 1, b"0123456789", &mut out);
    enc.encode_message(6, 9, 1066, 1, b"0123456789", &mut out); // fmt3: delta matches cache

    let mut dec = ChunkDecoder::new();
    dec.push_bytes(&out);
    let mut timestamps = Vec::new();
    while let Some((_, ts, payload)) = dec.poll_message().unwrap() {
        assert_eq!(payload, b"0123456789");
        timestamps.push(ts);
    }
    assert_eq!(timestamps, vec![1000, 1033, 1066]);
}

#[test]
fn extended_timestamp_round_trips() {
    let mut enc = ChunkEncoder::new(128);
    let mut out = Vec::new();
    let big_ts = 0x0100_0000; // exceeds the 0xFFFFFF escape threshold
    enc.encode_message(3, 20, big_ts, 0, b"payload", &mut out);

    let mut dec = ChunkDecoder::new();
    dec.push_bytes(&out);
    let (_, ts, payload) = dec.poll_message().unwrap().unwrap();
    assert_eq!(ts, big_ts);
    assert_eq!(payload, b"payload");
}

#[test]
fn interleaved_chunk_streams_reassemble_independently() {
    let mut enc = ChunkEncoder::new(8);
    let mut out = Vec::new();
    // Interleave two messages on different csids manually by encoding into separate buffers
    // then splicing their bytes, simulating a real interleaved wire stream.
    let mut a = Vec::new();
    enc.encode_message(4, 9, 100, 1, b"AAAAAAAAAAAAAAAA", &mut a); // video, 16 bytes, 2 fragments
    let mut b = Vec::new();
    enc.encode_message(5, 8, 200, 1, b"BBBBBBBB", &mut b); // audio, 8 bytes, 1 fragment

    // Splice: first fragment of A, all of B, then remaining fragment(s) of A.
    // a's first chunk is basic(1)+fmt0(11)+8 payload = 20 bytes.
    out.extend_from_slice(&a[..20]);
    out.extend_from_slice(&b);
    out.extend_from_slice(&a[20..]);

    let mut dec = ChunkDecoder::new();
    dec.set_chunk_size(8); // matches the encoder; real streams sync this via Set Chunk Size
    dec.push_bytes(&out);
    let mut messages = Vec::new();
    while let Some(msg) = dec.poll_message().unwrap() {
        messages.push(msg);
    }
    assert_eq!(messages.len(), 2);
    assert!(
        messages
            .iter()
            .any(|(t, ts, p)| *t == 8 && *ts == 200 && p == b"BBBBBBBB")
    );
    assert!(
        messages
            .iter()
            .any(|(t, ts, p)| *t == 9 && *ts == 100 && p == b"AAAAAAAAAAAAAAAA")
    );
}

#[test]
fn set_chunk_size_message_updates_decoder_fragmentation() {
    let mut enc = ChunkEncoder::new(128);
    let mut out = Vec::new();
    // Set Chunk Size protocol control message: type 1, 4-byte BE payload.
    enc.encode_message(2, 1, 0, 0, &64u32.to_be_bytes(), &mut out);
    enc.set_chunk_size(64);
    enc.encode_message(4, 9, 10, 1, &[7u8; 100], &mut out);

    let mut dec = ChunkDecoder::new();
    dec.push_bytes(&out);
    let (t0, _, p0) = dec.poll_message().unwrap().unwrap();
    assert_eq!(t0, 1);
    assert_eq!(p0, 64u32.to_be_bytes());
    let (t1, _, p1) = dec.poll_message().unwrap().unwrap();
    assert_eq!(t1, 9);
    assert_eq!(p1, vec![7u8; 100]);
}
