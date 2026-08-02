//! Unit tests for the `WebM` demuxer (sibling of `demux.rs`).
//!
//! Uses `super::*` (white-box) to reach private fields/helpers; the hand-built
//! byte fixtures mirror the same element subset as `tests/demux_minimal_webm.rs`
//! (kept independent — test-only duplication, not production code).

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;
use crate::ids;

/// Encode an element size VINT (marker stripped), smallest length that fits.
fn enc_size(value: u64) -> Vec<u8> {
    for len in 1u64..=8 {
        let data_bits = 7 * len;
        let max = (1u64 << data_bits) - 2; // reserve all-1s for "unknown"
        if value <= max {
            let full = value | (1u64 << data_bits);
            let be = full.to_be_bytes();
            return be[8 - len as usize..].to_vec();
        }
    }
    Vec::new() // unreachable for any realistic test payload size
}

/// Build one EBML element: id bytes + encoded size + payload.
fn elem(id: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(id);
    out.extend_from_slice(&enc_size(payload.len() as u64));
    out.extend_from_slice(payload);
    out
}

fn uint_payload(v: u64, len: usize) -> Vec<u8> {
    let be = v.to_be_bytes();
    be[8 - len..].to_vec()
}

/// A minimal, valid `WebM` byte stream: header (skipped) + Segment{Info,
/// Tracks{one video track}, Cluster{one `SimpleBlock` with the given flags
/// byte}}.
fn minimal_webm() -> Vec<u8> {
    build_webm(0x80) // keyframe, no lacing
}

/// `Tracks` with one video `TrackEntry` (`TrackNumber` 1, `V_VP9`, 1280x720).
fn tracks_with_one_video_track() -> Vec<u8> {
    let video = {
        let w = elem(&[0xB0], &uint_payload(1280, 2)); // PixelWidth
        let h = elem(&[0xBA], &uint_payload(720, 2)); // PixelHeight
        let mut body = Vec::new();
        body.extend_from_slice(&w);
        body.extend_from_slice(&h);
        elem(&[0xE0], &body) // Video
    };
    let track_entry = {
        let num = elem(&[0xD7], &uint_payload(1, 1)); // TrackNumber = 1
        let typ = elem(&[0x83], &uint_payload(1, 1)); // TrackType = video
        let codec = elem(&[0x86], b"V_VP9"); // CodecID
        let mut body = Vec::new();
        body.extend_from_slice(&num);
        body.extend_from_slice(&typ);
        body.extend_from_slice(&codec);
        body.extend_from_slice(&video);
        elem(&[0xAE], &body) // TrackEntry
    };
    elem(&[0x16, 0x54, 0xAE, 0x6B], &track_entry) // Tracks
}

/// A full `WebM` byte stream: header (skipped) + Segment{Info, Tracks{one
/// video track}, Cluster{Timecode, `cluster_child`}}.
fn build_webm_with_cluster_child(cluster_child: &[u8]) -> Vec<u8> {
    let info = elem(&[0x2A, 0xD7, 0xB1], &uint_payload(1_000_000, 3)); // TimecodeScale
    let tracks = tracks_with_one_video_track();
    let cluster = {
        let timecode = elem(&[0xE7], &uint_payload(0, 1)); // Timecode = 0
        let mut body = Vec::new();
        body.extend_from_slice(&timecode);
        body.extend_from_slice(cluster_child);
        elem(&[0x1F, 0x43, 0xB6, 0x75], &body) // Cluster
    };
    let mut segment_body = Vec::new();
    segment_body.extend_from_slice(&info);
    segment_body.extend_from_slice(&tracks);
    segment_body.extend_from_slice(&cluster);
    let segment = elem(&[0x18, 0x53, 0x80, 0x67], &segment_body); // Segment
    let header = elem(&[0x1A, 0x45, 0xDF, 0xA3], &[]); // EBML header (empty, skipped)

    let mut out = Vec::new();
    out.extend_from_slice(&header);
    out.extend_from_slice(&segment);
    out
}

/// A full `WebM` byte stream with a single `SimpleBlock` built from a raw,
/// already-encoded block body (track number VINT + timecode + flags + payload/lace bytes).
fn build_webm_with_block_body(block_body: &[u8]) -> Vec<u8> {
    let simple_block = elem(&[0xA3], block_body);
    build_webm_with_cluster_child(&simple_block)
}

fn build_webm(simple_block_flags: u8) -> Vec<u8> {
    let mut block_body = Vec::new();
    block_body.push(0x81); // track number VINT = 1 (1-byte)
    block_body.extend_from_slice(&0i16.to_be_bytes()); // relative timecode
    block_body.push(simple_block_flags);
    block_body.extend_from_slice(&[1, 2, 3]); // frame payload
    build_webm_with_block_body(&block_body)
}

#[test]
fn track_and_frame_extracted_single_push() {
    let mut d = Demuxer::new();
    d.push_bytes(&minimal_webm());

    let tracks = d.streams();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].track_number, 1);
    assert!(tracks[0].is_video());
    assert_eq!(tracks[0].codec_id, "V_VP9");
    assert_eq!(tracks[0].width, 1280);
    assert_eq!(tracks[0].height, 720);

    let frame = d.poll_frame().expect("one SimpleBlock frame");
    assert_eq!(frame.track_number, 1);
    assert_eq!(frame.timecode, 0);
    assert!(frame.is_keyframe);
    assert_eq!(&frame.payload[..], &[1, 2, 3]);
    assert!(d.poll_frame().is_none());

    assert_eq!(d.time_base(), Rational::new(1_000_000, 1_000_000_000));
}

#[test]
fn chunked_feed_matches_single_push() {
    let bytes = minimal_webm();
    let mut d = Demuxer::new();
    for byte in &bytes {
        d.push_bytes(std::slice::from_ref(byte));
    }
    assert_eq!(d.streams().len(), 1);
    let frame = d.poll_frame().expect("frame after chunked feed");
    assert_eq!(&frame.payload[..], &[1, 2, 3]);
}

#[test]
fn truncated_input_never_panics() {
    let bytes = minimal_webm();
    let mut d = Demuxer::new();
    d.push_bytes(&bytes[..bytes.len() / 2]);
    // No panic; partial structure may or may not have produced the track yet.
    let _ = d.streams();
    let _ = d.poll_frame();
}

#[test]
fn reserved_vint_halts_without_panic() {
    let mut d = Demuxer::new();
    d.push_bytes(&[0x00, 0x00, 0x00, 0x00]);
    assert!(d.streams().is_empty());
    assert!(d.poll_frame().is_none());
    // Further bytes after a halt must not panic either.
    d.push_bytes(&[0xAE, 0x01, 0x00]);
    assert!(d.streams().is_empty());
}

#[test]
fn xiph_laced_simple_block_decodes_all_sub_frames() {
    // Real 2-frame Xiph lace: frame_count-1=1, one explicit size (2), then
    // frame 1 = 2 bytes, frame 2 = whatever remains (3 bytes).
    let mut block_body = Vec::new();
    block_body.push(0x81); // track number VINT = 1
    block_body.extend_from_slice(&0i16.to_be_bytes()); // relative timecode
    block_body.push(0x82); // keyframe + Xiph lacing (bits 01)
    block_body.push(1); // frame_count - 1 = 1 -> 2 frames
    block_body.push(2); // frame 1 size = 2
    block_body.extend_from_slice(&[0xAA, 0xBB]); // frame 1 data
    block_body.extend_from_slice(&[0xCC, 0xDD, 0xEE]); // frame 2 data (remainder)

    let bytes = build_webm_with_block_body(&block_body);
    let mut d = Demuxer::new();
    d.push_bytes(&bytes);
    assert_eq!(d.streams().len(), 1, "track metadata still parses");

    let first = d.poll_frame().expect("first laced sub-frame");
    assert_eq!(&first.payload[..], &[0xAA, 0xBB]);
    assert!(first.is_keyframe);
    let second = d.poll_frame().expect("second laced sub-frame");
    assert_eq!(&second.payload[..], &[0xCC, 0xDD, 0xEE]);
    assert_eq!(
        second.timecode, first.timecode,
        "laced sub-frames share one timecode"
    );
    assert!(d.poll_frame().is_none());
}

#[test]
fn malformed_lace_drops_cleanly_without_panic() {
    // Claims Xiph lacing but the payload is too short for any valid lace.
    let mut block_body = Vec::new();
    block_body.push(0x81);
    block_body.extend_from_slice(&0i16.to_be_bytes());
    block_body.push(0x82); // keyframe + Xiph lacing
    block_body.extend_from_slice(&[1, 2, 3]); // not a well-formed lace

    let bytes = build_webm_with_block_body(&block_body);
    let mut d = Demuxer::new();
    d.push_bytes(&bytes);
    assert_eq!(d.streams().len(), 1);
    assert!(
        d.poll_frame().is_none(),
        "malformed lace is dropped, not decoded"
    );
}

#[test]
fn block_group_with_duration_and_no_reference_block_is_keyframe() {
    let block = {
        let mut body = Vec::new();
        body.push(0x81); // track number = 1
        body.extend_from_slice(&0i16.to_be_bytes());
        body.push(0x00); // flags: no lacing (Block's top bit is reserved, not keyframe)
        body.extend_from_slice(&[9, 9, 9]);
        elem(&[0xA1], &body) // Block
    };
    let duration = elem(&[0x9B], &uint_payload(33, 1)); // BlockDuration
    let mut group_body = Vec::new();
    group_body.extend_from_slice(&block);
    group_body.extend_from_slice(&duration);
    let block_group = elem(&[0xA0], &group_body); // BlockGroup

    let bytes = build_webm_with_cluster_child(&block_group);
    let mut d = Demuxer::new();
    d.push_bytes(&bytes);

    let frame = d.poll_frame().expect("BlockGroup frame");
    assert_eq!(&frame.payload[..], &[9, 9, 9]);
    assert!(frame.is_keyframe, "no ReferenceBlock -> keyframe");
    assert_eq!(frame.duration_ticks, Some(33));
}

#[test]
fn block_group_with_reference_block_is_not_keyframe() {
    let block = {
        let mut body = Vec::new();
        body.push(0x81);
        body.extend_from_slice(&0i16.to_be_bytes());
        body.push(0x00);
        body.extend_from_slice(&[7]);
        elem(&[0xA1], &body)
    };
    let reference_block = elem(&[0xFB], &[0xDD]); // ReferenceBlock — presence matters, value doesn't
    let mut group_body = Vec::new();
    group_body.extend_from_slice(&block);
    group_body.extend_from_slice(&reference_block);
    let block_group = elem(&[0xA0], &group_body);

    let bytes = build_webm_with_cluster_child(&block_group);
    let mut d = Demuxer::new();
    d.push_bytes(&bytes);

    let frame = d.poll_frame().expect("BlockGroup frame");
    assert!(
        !frame.is_keyframe,
        "ReferenceBlock present -> not a keyframe"
    );
    assert_eq!(frame.duration_ticks, None);
}

#[test]
fn audio_track_fields_populate_sample_rate_and_channels() {
    let sample_rate_payload = 48_000.0f64.to_be_bytes().to_vec();
    let audio = {
        let sr = elem(&[0xB5], &sample_rate_payload); // SamplingFrequency (8-byte float)
        let ch = elem(&[0x9F], &uint_payload(2, 1)); // Channels
        let mut body = Vec::new();
        body.extend_from_slice(&sr);
        body.extend_from_slice(&ch);
        elem(&[0xE1], &body) // Audio
    };
    let track_entry = {
        let num = elem(&[0xD7], &uint_payload(2, 1));
        let typ = elem(&[0x83], &uint_payload(2, 1)); // TrackType = audio
        let codec = elem(&[0x86], b"A_OPUS");
        let mut body = Vec::new();
        body.extend_from_slice(&num);
        body.extend_from_slice(&typ);
        body.extend_from_slice(&codec);
        body.extend_from_slice(&audio);
        elem(&[0xAE], &body)
    };
    let tracks = elem(&[0x16, 0x54, 0xAE, 0x6B], &track_entry);
    let segment = elem(&[0x18, 0x53, 0x80, 0x67], &tracks);
    let header = elem(&[0x1A, 0x45, 0xDF, 0xA3], &[]);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(&segment);

    let mut d = Demuxer::new();
    d.push_bytes(&bytes);
    let track = &d.streams()[0];
    assert!((track.sample_rate - 48_000.0).abs() < f64::EPSILON);
    assert_eq!(track.channels, 2);
}

#[test]
fn audio_track_defaults_when_fields_absent() {
    let bytes = minimal_webm(); // video track, no Audio element at all
    let mut d = Demuxer::new();
    d.push_bytes(&bytes);
    let track = &d.streams()[0];
    assert!((track.sample_rate - 8000.0).abs() < f64::EPSILON);
    assert_eq!(track.channels, 1);
}

#[test]
fn cues_and_seek_head_populate() {
    let cue_point = {
        let time = elem(&[0xB3], &uint_payload(5, 1)); // CueTime
        let track_positions = {
            let track = elem(&[0xF7], &uint_payload(1, 1)); // CueTrack
            let pos = elem(&[0xF1], &uint_payload(1234, 2)); // CueClusterPosition
            let mut body = Vec::new();
            body.extend_from_slice(&track);
            body.extend_from_slice(&pos);
            elem(&[0xB7], &body)
        };
        let mut body = Vec::new();
        body.extend_from_slice(&time);
        body.extend_from_slice(&track_positions);
        elem(&[0xBB], &body)
    };
    let cues = elem(&[0x1C, 0x53, 0xBB, 0x6B], &cue_point);

    let seek = {
        let seek_id = elem(&[0x53, 0xAB], &[0x16, 0x54, 0xAE, 0x6B]); // SeekID -> Tracks
        let seek_position = elem(&[0x53, 0xAC], &uint_payload(42, 1));
        let mut body = Vec::new();
        body.extend_from_slice(&seek_id);
        body.extend_from_slice(&seek_position);
        elem(&[0x4D, 0xBB], &body)
    };
    let seek_head = elem(&[0x11, 0x4D, 0x9B, 0x74], &seek);

    let mut segment_body = Vec::new();
    segment_body.extend_from_slice(&seek_head);
    segment_body.extend_from_slice(&cues);
    let segment = elem(&[0x18, 0x53, 0x80, 0x67], &segment_body);
    let header = elem(&[0x1A, 0x45, 0xDF, 0xA3], &[]);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(&segment);

    let mut d = Demuxer::new();
    d.push_bytes(&bytes);

    assert_eq!(d.cues().len(), 1);
    assert_eq!(d.cues()[0].time_ticks, 5);
    assert_eq!(d.cues()[0].cluster_position, 1234);

    assert_eq!(d.seek_head().len(), 1);
    assert_eq!(d.seek_head()[0].id, ids::TRACKS);
    assert_eq!(d.seek_head()[0].position, 42);
}

#[test]
fn read_uint_rejects_over_long_body() {
    assert_eq!(read_uint(&[0u8; 9]), None);
    assert_eq!(read_uint(&[0, 0, 0, 0, 0, 0, 0, 5]), Some(5));
    assert_eq!(read_uint(&[]), Some(0));
}

#[test]
fn is_descend_master_matches_expected_ids() {
    assert!(ids::is_descend_master(ids::SEGMENT));
    assert!(ids::is_descend_master(ids::CLUSTER));
    assert!(ids::is_descend_master(ids::TRACK_ENTRY));
    assert!(!ids::is_descend_master(ids::SIMPLE_BLOCK));
    assert!(!ids::is_descend_master(ids::EBML_HEADER));
}
