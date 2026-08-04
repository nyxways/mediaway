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

/// Hand-build one raw Ogg page with a valid CRC (the facade's own `Muxer`
/// only ever emits one packet per page, so multi-packet pages need building
/// by hand to exercise per-packet position back-computation).
fn build_page(
    serial: u32,
    seq: u32,
    granule: i64,
    flags: u8,
    segs: &[u8],
    payload: &[u8],
) -> Vec<u8> {
    let mut page = Vec::new();
    page.extend_from_slice(b"OggS");
    page.push(0);
    page.push(flags);
    page.extend_from_slice(&granule.to_le_bytes());
    page.extend_from_slice(&serial.to_le_bytes());
    page.extend_from_slice(&seq.to_le_bytes());
    page.extend_from_slice(&0u32.to_le_bytes()); // CRC placeholder
    page.push(u8::try_from(segs.len()).unwrap_or(u8::MAX));
    page.extend_from_slice(segs);
    page.extend_from_slice(payload);
    let crc = ogg_core::crc32_ogg(&page);
    page[22..26].copy_from_slice(&crc.to_le_bytes());
    page
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

#[test]
fn opus_per_packet_pts_back_computed_from_page_granule() {
    // Real encoders pack many packets into one page; all of them carry the
    // page granule (the end of the *last* one) in the core. The facade must
    // back-compute each packet's own end position from TOC durations.
    let head = opus_head(2);
    let head_seg = u8::try_from(head.len()).unwrap_or(u8::MAX);
    let page0 = build_page(7, 0, 0, 0x02, &[head_seg], &head); // OpusHead

    let toc = 0xFCu8; // config 31 -> FB CELT 20 ms frame, stereo, 1 frame -> 960 samples
    let audio = [toc, 0xAA, 0xBB, 0xCC];
    let mut payload = Vec::new();
    payload.extend_from_slice(&audio);
    payload.extend_from_slice(&audio);
    let page1 = build_page(7, 1, 1920, 0x00, &[4, 4], &payload); // granule = 2 x 960

    let mut demux = Demuxer::new();
    demux.push_bytes(&page0);
    demux.push_bytes(&page1);

    let p0 = demux.poll_packet().expect("audio packet 0");
    assert_eq!(
        p0.pts, 960,
        "first packet ends at granule minus second's duration"
    );
    assert_eq!(p0.duration, 960);
    let p1 = demux.poll_packet().expect("audio packet 1");
    assert_eq!(p1.pts, 1920, "last packet ends at the page granule");
    assert_eq!(p1.duration, 960);
    assert!(demux.poll_packet().is_none());
}
