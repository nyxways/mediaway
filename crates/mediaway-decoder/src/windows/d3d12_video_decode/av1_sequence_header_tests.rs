//! Pure sans-io unit tests for [`super::parse_sequence_header`] against a hand-built
//! fixture matching `mediaway-encoder-windows`'s `bitstream_av1.rs::write_sequence_header`'s
//! own field shape (constructed independently here, not by calling the encoder crate — a
//! real sans-io round-trip test, not a tautology, per ADR-0005 § Test plan).

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use super::parse_sequence_header;
use crate::DecodeError;

/// MSB-first bit writer mirroring [`mediaway_sw::h264::BitReader`]'s own bit order — test
/// fixture construction only.
#[derive(Default)]
struct BitWriter {
    bytes: Vec<u8>,
    bit_pos: usize,
}

impl BitWriter {
    fn write_bit(&mut self, bit: u32) {
        let byte_index = self.bit_pos / 8;
        if byte_index >= self.bytes.len() {
            self.bytes.push(0);
        }
        if bit != 0 {
            let shift = 7 - (self.bit_pos % 8);
            self.bytes[byte_index] |= 1 << shift;
        }
        self.bit_pos += 1;
    }

    fn write_bits(&mut self, value: u32, count: u32) {
        for i in (0..count).rev() {
            self.write_bit((value >> i) & 1);
        }
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

/// Every field this test fixture may vary, defaulting to a shape
/// [`parse_sequence_header`] must accept.
struct Ov {
    seq_profile: u32,
    reduced_still_picture_header: u32,
    timing_info_present_flag: u32,
    initial_display_delay_present_flag: u32,
    operating_points_cnt_minus_1: u32,
    frame_id_numbers_present_flag: u32,
    seq_force_screen_content_tools: u32,
    enable_superres: u32,
    enable_cdef: u32,
    enable_restoration: u32,
    high_bitdepth: u32,
    mono_chrome: u32,
    color_description_present_flag: u32,
    color_primaries: u32,
    transfer_characteristics: u32,
    matrix_coefficients: u32,
    film_grain_params_present: u32,
}

impl Default for Ov {
    fn default() -> Self {
        Self {
            seq_profile: 0,
            reduced_still_picture_header: 0,
            timing_info_present_flag: 0,
            initial_display_delay_present_flag: 0,
            operating_points_cnt_minus_1: 0,
            frame_id_numbers_present_flag: 0,
            seq_force_screen_content_tools: 0,
            enable_superres: 0,
            enable_cdef: 0,
            enable_restoration: 0,
            high_bitdepth: 0,
            mono_chrome: 0,
            color_description_present_flag: 0,
            color_primaries: 2,
            transfer_characteristics: 2,
            matrix_coefficients: 2,
            film_grain_params_present: 0,
        }
    }
}

const WIDTH: u32 = 352;
const HEIGHT: u32 = 288;

/// Build a `sequence_header_obu()` payload matching [`super::parse_sequence_header`]'s
/// exact read order, with `ov` controlling the fields under test. Every other field takes
/// the accepted, `KEY_FRAME`-only-scope shape (mirrors `bitstream_av1.rs::
/// write_sequence_header`'s own all-fixed field choices for the fields this module also
/// hardcodes).
fn build(ov: &Ov) -> Vec<u8> {
    let mut w = BitWriter::default();
    w.write_bits(ov.seq_profile, 3);
    w.write_bit(0); // still_picture
    w.write_bit(ov.reduced_still_picture_header);
    w.write_bit(ov.timing_info_present_flag);
    w.write_bit(ov.initial_display_delay_present_flag);
    w.write_bits(ov.operating_points_cnt_minus_1, 5);
    w.write_bits(0, 12); // operating_point_idc[0]
    w.write_bits(0, 5); // seq_level_idx[0] (<= 7, no seq_tier bit)

    let width_bits = 32 - (WIDTH - 1).leading_zeros();
    let height_bits = 32 - (HEIGHT - 1).leading_zeros();
    w.write_bits(width_bits - 1, 4);
    w.write_bits(height_bits - 1, 4);
    w.write_bits(WIDTH - 1, width_bits);
    w.write_bits(HEIGHT - 1, height_bits);

    w.write_bit(ov.frame_id_numbers_present_flag);
    w.write_bit(0); // use_128x128_superblock
    w.write_bit(0); // enable_filter_intra
    w.write_bit(0); // enable_intra_edge_filter
    w.write_bit(0); // enable_interintra_compound
    w.write_bit(0); // enable_masked_compound
    w.write_bit(0); // enable_warped_motion
    w.write_bit(0); // enable_dual_filter
    w.write_bit(0); // enable_order_hint (0 -> jnt_comp/ref_frame_mvs not read)
    w.write_bit(0); // seq_choose_screen_content_tools
    w.write_bits(ov.seq_force_screen_content_tools, 1);
    // enable_order_hint == 0 -> order_hint_bits_minus_1 not read.

    w.write_bit(ov.enable_superres);
    w.write_bit(ov.enable_cdef);
    w.write_bit(ov.enable_restoration);

    // color_config()
    w.write_bit(ov.high_bitdepth);
    w.write_bit(ov.mono_chrome);
    w.write_bit(ov.color_description_present_flag);
    if ov.color_description_present_flag != 0 {
        w.write_bits(ov.color_primaries, 8);
        w.write_bits(ov.transfer_characteristics, 8);
        w.write_bits(ov.matrix_coefficients, 8);
    }
    let is_identity_branch =
        ov.color_primaries == 1 && ov.transfer_characteristics == 13 && ov.matrix_coefficients == 0;
    if !is_identity_branch {
        w.write_bit(0); // color_range
        w.write_bits(0, 2); // chroma_sample_position (subsampling always (1,1) here)
    }
    w.write_bit(0); // separate_uv_delta_q

    w.write_bit(ov.film_grain_params_present);

    w.finish()
}

#[test]
fn accepts_the_encoder_shaped_fixture() {
    let bytes = build(&Ov::default());
    let sh = parse_sequence_header(&bytes).unwrap();
    assert_eq!(sh.max_frame_width, WIDTH);
    assert_eq!(sh.max_frame_height, HEIGHT);
    assert_eq!(sh.order_hint_bits, 0);
    assert!(!sh.enable_order_hint);
    assert!(!sh.separate_uv_delta_q);
}

#[test]
fn rejects_non_main_profile() {
    let bytes = build(&Ov {
        seq_profile: 1,
        ..Ov::default()
    });
    assert!(matches!(
        parse_sequence_header(&bytes),
        Err(DecodeError::Unsupported)
    ));
}

#[test]
fn rejects_reduced_still_picture_header() {
    let bytes = build(&Ov {
        reduced_still_picture_header: 1,
        ..Ov::default()
    });
    assert!(matches!(
        parse_sequence_header(&bytes),
        Err(DecodeError::Unsupported)
    ));
}

#[test]
fn rejects_timing_info_present_flag() {
    let bytes = build(&Ov {
        timing_info_present_flag: 1,
        ..Ov::default()
    });
    assert!(matches!(
        parse_sequence_header(&bytes),
        Err(DecodeError::Unsupported)
    ));
}

#[test]
fn rejects_initial_display_delay_present_flag() {
    let bytes = build(&Ov {
        initial_display_delay_present_flag: 1,
        ..Ov::default()
    });
    assert!(matches!(
        parse_sequence_header(&bytes),
        Err(DecodeError::Unsupported)
    ));
}

#[test]
fn rejects_multiple_operating_points() {
    let bytes = build(&Ov {
        operating_points_cnt_minus_1: 1,
        ..Ov::default()
    });
    assert!(matches!(
        parse_sequence_header(&bytes),
        Err(DecodeError::Unsupported)
    ));
}

#[test]
fn rejects_frame_id_numbers_present_flag() {
    let bytes = build(&Ov {
        frame_id_numbers_present_flag: 1,
        ..Ov::default()
    });
    assert!(matches!(
        parse_sequence_header(&bytes),
        Err(DecodeError::Unsupported)
    ));
}

#[test]
fn rejects_screen_content_tools() {
    let bytes = build(&Ov {
        seq_force_screen_content_tools: 1,
        ..Ov::default()
    });
    assert!(matches!(
        parse_sequence_header(&bytes),
        Err(DecodeError::Unsupported)
    ));
}

#[test]
fn rejects_enable_superres() {
    let bytes = build(&Ov {
        enable_superres: 1,
        ..Ov::default()
    });
    assert!(matches!(
        parse_sequence_header(&bytes),
        Err(DecodeError::Unsupported)
    ));
}

#[test]
fn rejects_enable_cdef() {
    let bytes = build(&Ov {
        enable_cdef: 1,
        ..Ov::default()
    });
    assert!(matches!(
        parse_sequence_header(&bytes),
        Err(DecodeError::Unsupported)
    ));
}

#[test]
fn rejects_enable_restoration() {
    let bytes = build(&Ov {
        enable_restoration: 1,
        ..Ov::default()
    });
    assert!(matches!(
        parse_sequence_header(&bytes),
        Err(DecodeError::Unsupported)
    ));
}

#[test]
fn rejects_high_bitdepth() {
    let bytes = build(&Ov {
        high_bitdepth: 1,
        ..Ov::default()
    });
    assert!(matches!(
        parse_sequence_header(&bytes),
        Err(DecodeError::Unsupported)
    ));
}

#[test]
fn rejects_mono_chrome() {
    let bytes = build(&Ov {
        mono_chrome: 1,
        ..Ov::default()
    });
    assert!(matches!(
        parse_sequence_header(&bytes),
        Err(DecodeError::Unsupported)
    ));
}

#[test]
fn rejects_non_4_2_0_subsampling_via_identity_matrix_branch() {
    // CP_BT_709 && TC_SRGB && MC_IDENTITY forces (subsampling_x, subsampling_y) = (0, 0)
    // regardless of seq_profile == 0 (AV1 spec §5.5.2) — a real reachable-at-profile-0
    // non-4:2:0 case this module must reject.
    let bytes = build(&Ov {
        color_description_present_flag: 1,
        color_primaries: 1,
        transfer_characteristics: 13,
        matrix_coefficients: 0,
        ..Ov::default()
    });
    assert!(matches!(
        parse_sequence_header(&bytes),
        Err(DecodeError::Unsupported)
    ));
}

#[test]
fn rejects_film_grain_params_present() {
    let bytes = build(&Ov {
        film_grain_params_present: 1,
        ..Ov::default()
    });
    assert!(matches!(
        parse_sequence_header(&bytes),
        Err(DecodeError::Unsupported)
    ));
}

#[test]
fn rejects_truncated_input() {
    let bytes = [0u8; 1];
    assert!(matches!(
        parse_sequence_header(&bytes),
        Err(DecodeError::InvalidInput)
    ));
}
