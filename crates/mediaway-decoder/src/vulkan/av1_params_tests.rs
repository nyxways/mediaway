#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;
use mediaway_common::{PixelFormat, Rational, VideoFrame, VideoFrameStorage};
use mediaway_sw::av1::{Av1Encoder, Av1EncoderConfig};

/// Real `rav1e`-encoded single `KEY_FRAME` OBU bytes, captured once via
/// `mediaway_sw::av1::Av1Encoder` and hardcoded here so this parser's tests
/// do not need to re-run the (slow-ish) software encoder on every test run —
/// `rav1e_keyframe_bytes_still_scan_the_same_obu_types` below cross-checks
/// this fixture stays representative of `rav1e`'s current real output shape.
/// Structure (confirmed via manual bit-level decode this implementation
/// pass, per `adr/vulkan/0002`'s open question #5): `OBU_TEMPORAL_DELIMITER`
/// (2 bytes) + `OBU_SEQUENCE_HEADER` (2-byte header + 10-byte payload) +
/// `OBU_FRAME` (2-byte header + 69-byte payload, `frame_type == KEY_FRAME`,
/// `base_q_idx == 66`, real segmentation + CDEF enabled).
const RAV1E_KEYFRAME_BYTES: &[u8] = &[
    0x12, 0x00, 0x0a, 0x0a, 0x00, 0x00, 0x00, 0xf9, 0x57, 0xff, 0xc4, 0x21, 0x50, 0x14, 0x32, 0x45,
    0x10, 0x02, 0x85, 0x1b, 0x8b, 0xfe, 0xf2, 0xea, 0x60, 0x00, 0x10, 0x00, 0x08, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x30, 0xc3, 0x0c, 0x10, 0x41, 0x10, 0xbb, 0x86, 0x2c, 0xfb, 0xad, 0x56,
    0x0f, 0x7b, 0x42, 0x1d, 0x12, 0x92, 0x8c, 0x51, 0xdc, 0xed, 0x14, 0x0e, 0x54, 0x05, 0xf4, 0x7d,
    0x54, 0xa8, 0x4f, 0x18, 0x10, 0x0c, 0xf6, 0x5d, 0x59, 0x03, 0x0b, 0xce, 0xd5, 0x1f, 0x33, 0xf6,
    0x07, 0x66, 0x55, 0x2e, 0x20,
];

fn fixture_bytes() -> Vec<u8> {
    RAV1E_KEYFRAME_BYTES.to_vec()
}

#[test]
fn read_leb128_single_byte() {
    assert_eq!(read_leb128(&[0x0a]).unwrap(), (10, 1));
}

#[test]
fn read_leb128_multi_byte() {
    // 0x81 0x01 = 0b1_0000001 continuation, 0b0_0000001 terminator ->
    // value = 1 | (1 << 7) = 129.
    assert_eq!(read_leb128(&[0x81, 0x01]).unwrap(), (129, 2));
}

#[test]
fn read_leb128_rejects_truncated_input() {
    assert!(read_leb128(&[0x81]).is_err());
}

#[test]
fn scan_obus_finds_td_seq_header_and_frame_in_real_rav1e_output() {
    let data = fixture_bytes();
    let obus = scan_obus(&data).unwrap();
    assert_eq!(obus.len(), 3);
    assert!(matches!(obus[0].obu_type, ObuType::TemporalDelimiter));
    assert!(obus[0].payload.is_empty());
    assert!(matches!(obus[1].obu_type, ObuType::SequenceHeader));
    assert_eq!(obus[1].payload.len(), 10);
    assert!(matches!(obus[2].obu_type, ObuType::Frame));
    assert_eq!(obus[2].payload.len(), 69);
}

#[test]
fn sequence_header_parses_real_rav1e_output() {
    let data = fixture_bytes();
    let obus = scan_obus(&data).unwrap();
    let seq = Av1SequenceHeader::parse(obus[1].payload).unwrap();
    assert_eq!(seq.seq_profile, 0);
    assert_eq!(seq.width(), 64);
    assert_eq!(seq.height(), 64);
    assert!(seq.enable_order_hint);
    assert_eq!(seq.order_hint_bits, 6);
    assert!(seq.enable_cdef);
    assert_eq!(seq.subsampling_x, 1);
    assert_eq!(seq.subsampling_y, 1);
    assert!(seq.separate_uv_delta_q);
}

#[test]
fn frame_header_parses_real_rav1e_key_frame() {
    let data = fixture_bytes();
    let obus = scan_obus(&data).unwrap();
    let seq = Av1SequenceHeader::parse(obus[1].payload).unwrap();
    let (frame_header, tile_layout) = parse_frame_header(obus[2].payload, &seq).unwrap();
    assert_eq!(frame_header.base_q_idx, 66);
    assert!(frame_header.segmentation_enabled);
    // Single-tile scope: the tile boundary must fall strictly inside the
    // OBU_FRAME payload, leaving real coded tile data behind it.
    assert!(tile_layout.tile_offset > 0);
    assert!(tile_layout.tile_size > 0);
    assert_eq!(
        tile_layout.tile_offset + tile_layout.tile_size,
        obus[2].payload.len() as u32
    );
}

#[test]
fn frame_header_builds_std_picture_info_without_panicking() {
    let data = fixture_bytes();
    let obus = scan_obus(&data).unwrap();
    let seq = Av1SequenceHeader::parse(obus[1].payload).unwrap();
    let (frame_header, _tile_layout) = parse_frame_header(obus[2].payload, &seq).unwrap();
    let mut optionals = Av1PictureInfoOptionals::new(&frame_header);
    optionals.finish();
    let picture_info = frame_header.to_std_picture_info(&optionals);
    assert_eq!(
        picture_info.frame_type,
        vulkanalia::vk::video::STD_VIDEO_AV1_FRAME_TYPE_KEY
    );
    let reference_info = frame_header.to_std_reference_info();
    assert_eq!(reference_info.OrderHint, frame_header.order_hint);
}

/// Real, freshly-encoded (not the hardcoded fixture) `rav1e` output — cross-
/// checks this module's parser against whatever the pinned `rav1e` version
/// actually emits right now, per `adr/vulkan/0002`'s own test-plan
/// requirement to inspect real encoder output rather than assume a shape.
#[test]
fn scan_obus_and_sequence_header_parse_a_freshly_encoded_rav1e_key_frame() {
    let width = 64u32;
    let height = 64u32;
    let mut config = Av1EncoderConfig::new(width, height, Rational::new(1, 30));
    config.speed = 10;
    let mut encoder = Av1Encoder::open(&config).unwrap();

    let y_len = (width * height) as usize;
    let chroma_len = y_len / 4;
    let mut data = vec![0u8; y_len + 2 * chroma_len];
    for (i, pixel) in data[..y_len].iter_mut().enumerate() {
        *pixel = (i % 256) as u8;
    }
    let frame = VideoFrame {
        pts: 0,
        duration: 1,
        width,
        height,
        format: PixelFormat::I420,
        storage: VideoFrameStorage::Cpu {
            data: mediaway_common::Bytes::from(data),
        },
    };
    encoder.push_frame(&frame).unwrap();
    encoder.flush().unwrap();

    let mut payload = None;
    while let Some(packet) = encoder.poll_packet().unwrap() {
        if packet.is_keyframe {
            payload = Some(packet.payload);
            break;
        }
    }
    let payload = payload.expect("expected at least one keyframe packet");

    let obus = scan_obus(&payload).unwrap();
    let seq_obu = obus
        .iter()
        .find(|obu| matches!(obu.obu_type, ObuType::SequenceHeader))
        .expect("expected an OBU_SEQUENCE_HEADER");
    let seq = Av1SequenceHeader::parse(seq_obu.payload).unwrap();
    assert_eq!(seq.width(), width);
    assert_eq!(seq.height(), height);

    let frame_obu = obus
        .iter()
        .find(|obu| matches!(obu.obu_type, ObuType::Frame))
        .expect(
            "expected an OBU_FRAME (this parser does not support split OBU_FRAME_HEADER + \
                  OBU_TILE_GROUP this round)",
        );
    let (_frame_header, tile_layout) = parse_frame_header(frame_obu.payload, &seq).unwrap();
    assert!(tile_layout.tile_size > 0);
}
