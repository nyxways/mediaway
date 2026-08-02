//! Tier 2 integration test: demux a small, hand-built in-memory `WebM` byte
//! buffer through the public `ebml_webm::Demuxer` API only.
//!
//! Element subset and known gaps: `adr/0001-ebml-vint-webm-schema-v1.md`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_possible_truncation,
    reason = "integration tests may unwrap; VINT lengths here are always <=8"
)]

use ebml_webm::Demuxer;

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
    Vec::new()
}

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

fn simple_block(track_number: u8, rel_tc: i16, flags: u8, frame: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.push(0x80 | track_number); // 1-byte track number VINT
    body.extend_from_slice(&rel_tc.to_be_bytes());
    body.push(flags);
    body.extend_from_slice(frame);
    elem(&[0xA3], &body)
}

/// One video track (`V_VP9`, 1280x720) and one Cluster with two keyframe
/// `SimpleBlock`s at timecodes 0 and 40 (ms, at the default 1ms `TimecodeScale`).
fn minimal_webm() -> Vec<u8> {
    let info = elem(&[0x2A, 0xD7, 0xB1], &uint_payload(1_000_000, 3));
    let video = {
        let w = elem(&[0xB0], &uint_payload(1280, 2));
        let h = elem(&[0xBA], &uint_payload(720, 2));
        elem(&[0xE0], &[w, h].concat())
    };
    let track_entry = {
        let num = elem(&[0xD7], &uint_payload(1, 1));
        let typ = elem(&[0x83], &uint_payload(1, 1));
        let codec = elem(&[0x86], b"V_VP9");
        elem(&[0xAE], &[num, typ, codec, video].concat())
    };
    let tracks = elem(&[0x16, 0x54, 0xAE, 0x6B], &track_entry);
    let cluster = {
        let timecode = elem(&[0xE7], &uint_payload(0, 1));
        let block_a = simple_block(1, 0, 0x80, &[1, 2, 3]);
        let block_b = simple_block(1, 40, 0x80, &[4, 5]);
        elem(
            &[0x1F, 0x43, 0xB6, 0x75],
            &[timecode, block_a, block_b].concat(),
        )
    };
    let segment = elem(&[0x18, 0x53, 0x80, 0x67], &[info, tracks, cluster].concat());
    let header = elem(&[0x1A, 0x45, 0xDF, 0xA3], &[]);
    [header, segment].concat()
}

#[test]
fn demuxes_track_metadata_and_frames() {
    let mut d = Demuxer::new();
    d.push_bytes(&minimal_webm());

    let tracks = d.streams();
    assert_eq!(tracks.len(), 1);
    let t = &tracks[0];
    assert_eq!(t.track_number, 1);
    assert!(t.is_video());
    assert_eq!(t.codec_id, "V_VP9");
    assert_eq!(t.width, 1280);
    assert_eq!(t.height, 720);

    let f1 = d.poll_frame().expect("first frame");
    assert_eq!(f1.timecode, 0);
    assert!(f1.is_keyframe);
    assert_eq!(&f1.payload[..], &[1, 2, 3]);

    let f2 = d.poll_frame().expect("second frame");
    assert_eq!(f2.timecode, 40);
    assert_eq!(&f2.payload[..], &[4, 5]);

    assert!(d.poll_frame().is_none());
}

#[test]
fn feeding_bytes_one_at_a_time_still_demuxes() {
    let bytes = minimal_webm();
    let mut d = Demuxer::new();
    for b in &bytes {
        d.push_bytes(std::slice::from_ref(b));
    }
    assert_eq!(d.streams().len(), 1);
    assert!(d.poll_frame().is_some());
    assert!(d.poll_frame().is_some());
}

#[test]
fn truncated_buffer_never_panics_and_yields_partial_or_no_data() {
    let bytes = minimal_webm();
    for cut in [1, 5, bytes.len() / 3, bytes.len() / 2, bytes.len() - 1] {
        let mut d = Demuxer::new();
        d.push_bytes(&bytes[..cut]);
        // Must not panic; results are best-effort and not asserted further.
        let _ = d.streams();
        let _ = d.poll_frame();
    }
}

#[test]
fn malformed_reserved_vint_never_panics() {
    let mut d = Demuxer::new();
    d.push_bytes(&[0x00; 16]);
    assert!(d.streams().is_empty());
    assert!(d.poll_frame().is_none());
}

#[test]
fn empty_input_never_panics() {
    let mut d = Demuxer::new();
    d.push_bytes(&[]);
    assert!(d.streams().is_empty());
    assert!(d.poll_frame().is_none());
}
