#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

/// Minimal MSB-first bit packer — same convention as `hevc_params_tests.rs`'s
/// own `BitWriter` (duplicated here to keep this test file self-contained).
struct BitWriter {
    bytes: Vec<u8>,
    cur: u8,
    nbits: u8,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            cur: 0,
            nbits: 0,
        }
    }

    fn push_bit(&mut self, bit: u32) {
        let bit_u8 = u8::from(bit & 1 == 1);
        self.cur = (self.cur << 1) | bit_u8;
        self.nbits += 1;
        if self.nbits == 8 {
            self.bytes.push(self.cur);
            self.cur = 0;
            self.nbits = 0;
        }
    }

    fn write_bits(&mut self, value: u32, count: u32) {
        for i in (0..count).rev() {
            self.push_bit((value >> i) & 1);
        }
    }

    fn finish(mut self) -> Vec<u8> {
        while self.nbits != 0 {
            self.push_bit(0);
        }
        self.bytes
    }
}

fn minimal_sequence_header() -> Av1SequenceHeader {
    Av1SequenceHeader {
        seq_profile: 0,
        frame_width_bits_minus_1: 5,
        frame_height_bits_minus_1: 5,
        max_frame_width_minus_1: 63,
        max_frame_height_minus_1: 63,
        use_128x128_superblock: false,
        enable_filter_intra: false,
        enable_intra_edge_filter: false,
        enable_interintra_compound: false,
        enable_masked_compound: false,
        enable_warped_motion: false,
        enable_dual_filter: false,
        enable_order_hint: false,
        enable_jnt_comp: false,
        enable_ref_frame_mvs: false,
        seq_force_screen_content_tools: 0,
        seq_force_integer_mv: 2, // SELECT_VALUE (av1_params.rs's own constant)
        order_hint_bits: 0,
        enable_superres: false,
        enable_cdef: false,
        enable_restoration: false,
        film_grain_params_present: false,
        subsampling_x: 1,
        subsampling_y: 1,
        separate_uv_delta_q: false,
        color_range: false,
    }
}

#[test]
fn show_existing_frame_is_rejected() {
    let mut writer = BitWriter::new();
    writer.push_bit(1); // show_existing_frame
    let payload = writer.finish();
    let seq = minimal_sequence_header();
    let err = parse_frame_header(&payload, &seq).unwrap_err();
    assert!(matches!(err, Av1ParamError::Unsupported { .. }));
}

#[test]
fn inter_frame_type_is_rejected() {
    let mut writer = BitWriter::new();
    writer.push_bit(0); // show_existing_frame
    writer.write_bits(1, 2); // frame_type = INTER_FRAME (1)
    let payload = writer.finish();
    let seq = minimal_sequence_header();
    let err = parse_frame_header(&payload, &seq).unwrap_err();
    assert!(matches!(err, Av1ParamError::Unsupported { .. }));
}

#[test]
fn show_frame_zero_is_rejected() {
    let mut writer = BitWriter::new();
    writer.push_bit(0); // show_existing_frame
    writer.write_bits(0, 2); // frame_type = KEY_FRAME (0)
    writer.push_bit(0); // show_frame = 0
    let payload = writer.finish();
    let seq = minimal_sequence_header();
    let err = parse_frame_header(&payload, &seq).unwrap_err();
    assert!(matches!(err, Av1ParamError::Unsupported { .. }));
}

#[test]
fn read_su_decodes_negative_value() {
    // su(7): the whole 7-bit field is `value` (MSB is the sign bit, not a
    // separate magnitude prefix); MSB set -> value - 2^7 = 106 - 128 = -22.
    let mut writer = BitWriter::new();
    writer.write_bits(0b1_101010, 7);
    let payload = writer.finish();
    let mut reader = BitReader::new(&payload);
    assert_eq!(read_su(&mut reader, 7).unwrap(), -22);
}

#[test]
fn read_su_decodes_positive_value() {
    let mut writer = BitWriter::new();
    writer.write_bits(0b0_000101, 7);
    let payload = writer.finish();
    let mut reader = BitReader::new(&payload);
    assert_eq!(read_su(&mut reader, 7).unwrap(), 5);
}

#[test]
fn read_delta_q_returns_zero_when_not_coded() {
    let mut writer = BitWriter::new();
    writer.push_bit(0); // delta_coded = 0
    let payload = writer.finish();
    let mut reader = BitReader::new(&payload);
    assert_eq!(read_delta_q(&mut reader).unwrap(), 0);
}

#[test]
fn tile_log2_smallest_k_such_that_shifted_blk_size_covers_target() {
    assert_eq!(tile_log2(1, 1), 0);
    assert_eq!(tile_log2(1, 2), 1);
    assert_eq!(tile_log2(1, 5), 3);
    assert_eq!(tile_log2(64, 1), 0);
}

#[test]
fn read_ns_zero_bits_for_n_equal_one() {
    let payload = BitWriter::new().finish();
    let mut reader = BitReader::new(&payload);
    assert_eq!(read_ns(&mut reader, 1).unwrap(), 0);
}
