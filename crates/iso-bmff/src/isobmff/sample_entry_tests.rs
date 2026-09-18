//! Unit tests for `vp09`/`vpcC` and `avc1`/`avcC` sample-entry write/parse.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "unit tests"
)]

use super::{parse_sample_entry, write_stsd};
use crate::isobmff::parse_header;
use crate::types::{Bytes, Codec, Rational, Track};

fn vp9_track(extra: Bytes) -> Track {
    Track {
        id: 0,
        codec: Codec::Vp9,
        time_base: Rational::new(1, 1000),
        width: 640,
        height: 480,
        extra_data: extra,
    }
}

fn h264_track() -> Track {
    Track {
        id: 0,
        codec: Codec::H264,
        time_base: Rational::new(1, 1000),
        width: 320,
        height: 240,
        extra_data: Bytes::new(),
    }
}

fn hevc_track(extra: Bytes) -> Track {
    Track {
        id: 0,
        codec: Codec::Hevc,
        time_base: Rational::new(1, 1000),
        width: 1920,
        height: 1080,
        extra_data: extra,
    }
}

fn av1_track(extra: Bytes) -> Track {
    Track {
        id: 0,
        codec: Codec::Av1,
        time_base: Rational::new(1, 1000),
        width: 3840,
        height: 2160,
        extra_data: extra,
    }
}

fn parse_stsd_entry(stsd: &[u8]) -> (u32, u32, Codec, Bytes) {
    let hdr = parse_header(stsd).expect("stsd header");
    let body = &stsd[hdr.header_len..hdr.size];
    let mut width = 0;
    let mut height = 0;
    let mut codec = Codec::H264;
    let mut extra = Bytes::new();
    let mut encryption = None;
    parse_sample_entry(
        &body[8..],
        &mut width,
        &mut height,
        &mut codec,
        &mut extra,
        &mut encryption,
    );
    assert!(encryption.is_none());
    (width, height, codec, extra)
}

#[test]
fn vp9_sample_entry_writes_vp09_not_avc1() {
    let mut buf = Vec::new();
    write_stsd(&mut buf, &vp9_track(Bytes::new()));
    assert!(buf.windows(4).any(|w| w == b"vp09"));
    assert!(!buf.windows(4).any(|w| w == b"avc1"));
}

#[test]
fn h264_sample_entry_still_writes_avc1() {
    let mut buf = Vec::new();
    write_stsd(&mut buf, &h264_track());
    assert!(buf.windows(4).any(|w| w == b"avc1"));
    assert!(!buf.windows(4).any(|w| w == b"vp09"));
}

#[test]
fn vp9_sample_entry_roundtrips_dimensions_and_codec() {
    let mut buf = Vec::new();
    write_stsd(&mut buf, &vp9_track(Bytes::new()));
    let (width, height, codec, extra) = parse_stsd_entry(&buf);

    assert_eq!(codec, Codec::Vp9);
    assert_eq!(width, 640);
    assert_eq!(height, 480);
    assert!(!extra.is_empty(), "placeholder vpcC should round-trip");
}

#[test]
fn vp9_sample_entry_reuses_demuxed_vpcc_payload() {
    let demuxed_vpcc = Bytes::from_static(&[1, 0, 0, 0, 2, 62, 0x8a, 1, 1, 1, 0, 0]);
    let mut buf = Vec::new();
    write_stsd(&mut buf, &vp9_track(demuxed_vpcc.clone()));
    let (_, _, codec, extra) = parse_stsd_entry(&buf);

    assert_eq!(codec, Codec::Vp9);
    assert_eq!(extra, demuxed_vpcc);
}

#[test]
fn hevc_sample_entry_writes_hvc1_not_avc1() {
    let mut buf = Vec::new();
    write_stsd(&mut buf, &hevc_track(Bytes::new()));
    assert!(buf.windows(4).any(|w| w == b"hvc1"));
    assert!(!buf.windows(4).any(|w| w == b"avc1"));
}

#[test]
fn hevc_sample_entry_roundtrips_dimensions_and_codec() {
    let mut buf = Vec::new();
    write_stsd(&mut buf, &hevc_track(Bytes::new()));
    let (width, height, codec, extra) = parse_stsd_entry(&buf);

    assert_eq!(codec, Codec::Hevc);
    assert_eq!(width, 1920);
    assert_eq!(height, 1080);
    assert!(!extra.is_empty(), "placeholder hvcC should round-trip");
}

#[test]
fn hevc_sample_entry_reuses_demuxed_hvcc_payload() {
    let demuxed_hvcc = Bytes::from_static(&[
        1, 1, 0x60, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x5A, 0xF0, 0, 0xFC, 0xFD, 0xF8, 0xF8, 0, 0, 0x03, 0,
    ]);
    let mut buf = Vec::new();
    write_stsd(&mut buf, &hevc_track(demuxed_hvcc.clone()));
    let (_, _, codec, extra) = parse_stsd_entry(&buf);

    assert_eq!(codec, Codec::Hevc);
    assert_eq!(extra, demuxed_hvcc);
}

#[test]
fn av1_sample_entry_writes_av01_not_avc1() {
    let mut buf = Vec::new();
    write_stsd(&mut buf, &av1_track(Bytes::new()));
    assert!(buf.windows(4).any(|w| w == b"av01"));
    assert!(!buf.windows(4).any(|w| w == b"avc1"));
}

#[test]
fn av1_sample_entry_roundtrips_dimensions_and_codec() {
    let mut buf = Vec::new();
    write_stsd(&mut buf, &av1_track(Bytes::new()));
    let (width, height, codec, extra) = parse_stsd_entry(&buf);

    assert_eq!(codec, Codec::Av1);
    assert_eq!(width, 3840);
    assert_eq!(height, 2160);
    assert!(!extra.is_empty(), "placeholder av1C should round-trip");
}

#[test]
fn av1_sample_entry_reuses_demuxed_av1c_payload() {
    let demuxed_av1c = Bytes::from_static(&[0x81, 0x08, 0x0C, 0]);
    let mut buf = Vec::new();
    write_stsd(&mut buf, &av1_track(demuxed_av1c.clone()));
    let (_, _, codec, extra) = parse_stsd_entry(&buf);

    assert_eq!(codec, Codec::Av1);
    assert_eq!(extra, demuxed_av1c);
}

/// `Opus` / `dOps` sample entry — [ADR-0005](../../adr/0005-opus-sample-entry.md).
///
/// Before that ADR, `Codec::Opus` shared the `mp4a`/`esds` branch with AAC, so an Opus
/// track was written with an `esds` declaring `objectTypeIndication` 0x40 (MPEG-4 AAC).
/// The file muxed cleanly and failed on playback. These tests pin the shape that replaced
/// it, and the first two exist specifically to fail if that branch ever comes back.
mod opus {
    use super::{parse_stsd_entry, write_stsd};
    use crate::types::{Bytes, Codec, Rational, Track};

    /// A real `OpusHead` (RFC 7845 § 5.1): 2 channels, pre-skip 312, 48 kHz input,
    /// unity gain, mapping family 0. Multi-byte fields are little-endian here and must
    /// come out big-endian in `dOps`.
    const OPUS_HEAD: &[u8] = &[
        b'O', b'p', b'u', b's', b'H', b'e', b'a', b'd', //
        1,    // Version
        2,    // ChannelCount
        0x38, 0x01, // PreSkip = 312, little-endian
        0x80, 0xbb, 0x00, 0x00, // InputSampleRate = 48000, little-endian
        0x00, 0x00, // OutputGain
        0,    // MappingFamily
    ];

    fn opus_track(extra: Bytes) -> Track {
        Track {
            id: 0,
            codec: Codec::Opus,
            time_base: Rational::new(1, 48_000),
            width: 0,
            height: 0,
            extra_data: extra,
        }
    }

    #[test]
    fn opus_sample_entry_writes_opus_dops_and_never_mp4a_esds() {
        let mut buf = Vec::new();
        write_stsd(&mut buf, &opus_track(Bytes::from(OPUS_HEAD.to_vec())));

        assert!(
            buf.windows(4).any(|w| w == b"Opus"),
            "expected an Opus sample entry"
        );
        assert!(buf.windows(4).any(|w| w == b"dOps"), "expected a dOps box");
        assert!(
            !buf.windows(4).any(|w| w == b"mp4a"),
            "Opus must not be written as an mp4a sample entry"
        );
        assert!(
            !buf.windows(4).any(|w| w == b"esds"),
            "the Opus spec forbids esds here; dOps replaces it"
        );
    }

    #[test]
    fn an_opus_track_no_longer_claims_to_be_aac() {
        // The regression this ADR exists for. `esds` encoded
        // `objectTypeIndication = 0x40` (MPEG-4 Audio) followed by streamType 0x15;
        // that byte pair over an Opus track is the exact mislabel that shipped.
        let mut buf = Vec::new();
        write_stsd(&mut buf, &opus_track(Bytes::from(OPUS_HEAD.to_vec())));
        assert!(
            !buf.windows(2).any(|w| w == [0x40, 0x15]),
            "found an AAC objectTypeIndication in an Opus sample entry"
        );
    }

    #[test]
    fn opus_head_round_trips_through_dops_with_byte_order_flipped() {
        let mut buf = Vec::new();
        write_stsd(&mut buf, &opus_track(Bytes::from(OPUS_HEAD.to_vec())));

        // dOps is big-endian, so pre-skip 312 is 0x01,0x38 on the wire — the reverse of
        // the OpusHead bytes above. Asserting this directly is what proves the conversion
        // is a real re-serialization and not a memcpy that happens to round-trip.
        let dops = buf
            .windows(4)
            .position(|w| w == b"dOps")
            .map(|i| &buf[i + 4..])
            .expect("dOps payload");
        assert_eq!(
            &dops[..11],
            &[
                0x01, 0x02, 0x01, 0x38, 0x00, 0x00, 0xbb, 0x80, 0x00, 0x00, 0x00
            ]
        );

        let (_, _, codec, extra) = parse_stsd_entry(&buf);
        assert_eq!(codec, Codec::Opus);
        assert_eq!(
            &extra[..],
            OPUS_HEAD,
            "demux must hand back an OpusHead, matching what webm stores as CodecPrivate"
        );
    }

    #[test]
    fn a_track_with_no_opus_head_gets_a_valid_placeholder() {
        // Same posture as VP9's VPCC_PLACEHOLDER: structurally valid so the file parses,
        // and a documented signaling gap rather than a truncated box.
        let mut buf = Vec::new();
        write_stsd(&mut buf, &opus_track(Bytes::new()));

        let (_, _, codec, extra) = parse_stsd_entry(&buf);
        assert_eq!(codec, Codec::Opus);
        assert_eq!(&extra[..8], b"OpusHead");
        assert_eq!(extra[9], 2, "placeholder is stereo");
        assert_eq!(
            u16::from_le_bytes([extra[10], extra[11]]),
            3840,
            "placeholder pre-skip is RFC 7845's recommended 80 ms at 48 kHz"
        );
    }

    #[test]
    fn a_multichannel_opus_head_carries_its_channel_mapping_table() {
        // MappingFamily != 0 appends StreamCount + CoupledCount + one byte per channel.
        // Dropping that tail would produce a dOps that parses but decodes to the wrong
        // channel layout — silent corruption, so it gets its own test.
        let mut head = OPUS_HEAD.to_vec();
        head[9] = 3; // ChannelCount
        head[18] = 1; // MappingFamily
        head.extend_from_slice(&[2, 1, 0, 1, 2]); // StreamCount, CoupledCount, mapping

        let mut buf = Vec::new();
        write_stsd(&mut buf, &opus_track(Bytes::from(head.clone())));

        let (_, _, codec, extra) = parse_stsd_entry(&buf);
        assert_eq!(codec, Codec::Opus);
        assert_eq!(
            &extra[..],
            &head[..],
            "the mapping table must survive both directions"
        );
    }

    #[test]
    fn a_truncated_opus_head_falls_back_instead_of_writing_a_short_box() {
        let mut buf = Vec::new();
        write_stsd(&mut buf, &opus_track(Bytes::from(OPUS_HEAD[..12].to_vec())));

        let (_, _, codec, extra) = parse_stsd_entry(&buf);
        assert_eq!(codec, Codec::Opus);
        assert_eq!(extra[9], 2, "fell back to the stereo placeholder");
    }
}
