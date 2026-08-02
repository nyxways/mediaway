//! Unit tests for the Ogg facade adapter (sibling of `ogg.rs`).

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

fn opus_head(channels: u8) -> Vec<u8> {
    let mut h = Vec::new();
    h.extend_from_slice(OPUS_HEAD_MAGIC);
    h.push(1); // version
    h.push(channels);
    h.extend_from_slice(&0u16.to_le_bytes()); // pre-skip
    h.extend_from_slice(&48_000u32.to_le_bytes()); // input sample rate (informational)
    h.extend_from_slice(&0i16.to_le_bytes()); // output gain
    h.push(0); // channel mapping family
    h
}

fn vorbis_ident(channels: u8, sample_rate: u32) -> Vec<u8> {
    let mut h = Vec::new();
    h.extend_from_slice(VORBIS_ID_MAGIC);
    h.extend_from_slice(&0u32.to_le_bytes()); // vorbis_version
    h.push(channels);
    h.extend_from_slice(&sample_rate.to_le_bytes());
    // bitrate_maximum/nominal/minimum (4 bytes each) + blocksize_0/1 byte +
    // framing_flag byte = 14 bytes, unused by `identify`.
    h.extend_from_slice(&[0u8; 14]);
    h
}

#[test]
fn identify_recognizes_opus_head() {
    let info = identify(&opus_head(2)).expect("OpusHead");
    assert_eq!(info.codec(), CodecKind::Opus);
    assert_eq!(info.sample_rate(), Some(48_000));
    assert_eq!(info.channels(), Some(2));
}

#[test]
fn identify_recognizes_vorbis_ident_header() {
    let info = identify(&vorbis_ident(1, 22_050)).expect("vorbis ident");
    assert_eq!(info.codec(), CodecKind::Vorbis);
    assert_eq!(info.sample_rate(), Some(22_050));
    assert_eq!(info.channels(), Some(1));
}

#[test]
fn identify_none_for_unrecognized_packet() {
    assert!(identify(b"not a codec header at all, just data").is_none());
}

#[test]
fn mux_then_demux_roundtrips_opus_stream() {
    let head = opus_head(2);
    let mut mux = Muxer::new(1);
    mux.push_packet(&Packet {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 0,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::copy_from_slice(&head),
    })
    .expect("push header");
    mux.push_packet(&Packet {
        stream_id: 0,
        pts: 960,
        dts: 960,
        duration: 0,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::from_static(&[1, 2, 3, 4]),
    })
    .expect("push audio packet");

    let mut bytes = Vec::new();
    mux.poll_bytes(&mut bytes);

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);

    let audio = demux.poll_packet().expect("audio packet");
    assert_eq!(&audio.payload[..], &[1, 2, 3, 4]);
    assert_eq!(audio.pts, 960);

    assert_eq!(demux.streams().len(), 1);
    assert_eq!(demux.streams()[0].codec(), CodecKind::Opus);
    assert_eq!(demux.streams()[0].channels(), Some(2));
}
