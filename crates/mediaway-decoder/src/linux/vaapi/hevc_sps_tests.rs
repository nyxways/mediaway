#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

/// Minimal bit-level writer producing an SPS-shaped RBSP for round-trip tests (mirrors this
/// crate's H.264 `sps_tests.rs`/`pps_tests.rs`'s writers; kept file-local per this workspace's
/// sibling-test convention).
#[derive(Default)]
struct BitWriter {
    bits: Vec<u8>,
}

impl BitWriter {
    fn push_bit(&mut self, bit: u8) {
        self.bits.push(bit & 1);
    }

    fn push_bits(&mut self, value: u32, count: u32) {
        for i in (0..count).rev() {
            self.push_bit(((value >> i) & 1) as u8);
        }
    }

    fn push_ue(&mut self, value: u32) {
        let value_plus1 = value + 1;
        let num_bits = 32 - value_plus1.leading_zeros();
        for _ in 0..num_bits - 1 {
            self.push_bit(0);
        }
        self.push_bits(value_plus1, num_bits);
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut out = vec![0u8; self.bits.len().div_ceil(8)];
        for (i, bit) in self.bits.iter().enumerate() {
            if *bit != 0 {
                out[i / 8] |= 1 << (7 - (i % 8));
            }
        }
        out
    }
}

/// Every knob a valid, minimal, this-crate-acceptable SPS RBSP can vary — defaults produce the
/// same shape [`valid_sps_rbsp`] builds.
struct SpsKnobs {
    sps_max_sub_layers_minus1: u32,
    chroma_format_idc: u32,
    scaling_list_enabled_flag: u32,
    pcm_enabled_flag: u32,
    num_short_term_ref_pic_sets: u32,
    long_term_ref_pics_present_flag: u32,
}

impl Default for SpsKnobs {
    fn default() -> Self {
        Self {
            sps_max_sub_layers_minus1: 0,
            chroma_format_idc: 1,
            scaling_list_enabled_flag: 0,
            pcm_enabled_flag: 0,
            num_short_term_ref_pic_sets: 0,
            long_term_ref_pics_present_flag: 0,
        }
    }
}

/// Builds a real, spec-ordered SPS RBSP (ITU-T H.265 § 7.3.2.2.1) with `knobs` — every field
/// this crate's own `HevcSps::parse` needs is given a distinct, non-default value so a field
/// transposition bug would be caught by [`parses_every_field_correctly`]'s assertions.
fn sps_rbsp(knobs: &SpsKnobs) -> Vec<u8> {
    let mut w = BitWriter::default();
    w.push_bits(0, 4); // sps_video_parameter_set_id
    w.push_bits(knobs.sps_max_sub_layers_minus1, 3);
    w.push_bit(1); // sps_temporal_id_nesting_flag

    // profile_tier_level(profilePresentFlag=1, maxNumSubLayersMinus1)
    w.push_bits(0, 2); // general_profile_space
    w.push_bit(0); // general_tier_flag
    w.push_bits(1, 5); // general_profile_idc: Main
    for _ in 0..32 {
        w.push_bit(0); // general_profile_compatibility_flag[i]
    }
    w.push_bit(1); // general_progressive_source_flag
    w.push_bit(0); // general_interlaced_source_flag
    w.push_bit(0); // general_non_packed_constraint_flag
    w.push_bit(1); // general_frame_only_constraint_flag
    w.push_bits(0, 32); // reserved
    w.push_bits(0, 12); // reserved
    w.push_bits(93, 8); // general_level_idc: Level 3.1
    for _ in 0..knobs.sps_max_sub_layers_minus1 {
        w.push_bit(0); // sub_layer_profile_present_flag[i]
        w.push_bit(0); // sub_layer_level_present_flag[i]
    }

    w.push_ue(0); // sps_seq_parameter_set_id
    w.push_ue(knobs.chroma_format_idc);
    if knobs.chroma_format_idc == 3 {
        w.push_bit(0); // separate_colour_plane_flag
    }
    w.push_ue(64); // pic_width_in_luma_samples
    w.push_ue(48); // pic_height_in_luma_samples
    w.push_bit(0); // conformance_window_flag
    w.push_ue(0); // bit_depth_luma_minus8
    w.push_ue(0); // bit_depth_chroma_minus8
    w.push_ue(4); // log2_max_pic_order_cnt_lsb_minus4 -> log2_max_pic_order_cnt_lsb == 8
    w.push_bit(0); // sps_sub_layer_ordering_info_present_flag
    for _ in 0..=knobs.sps_max_sub_layers_minus1 {
        w.push_ue(1); // sps_max_dec_pic_buffering_minus1[i] -> max_dec_pic_buffering == 2
        w.push_ue(0); // sps_max_num_reorder_pics[i]
        w.push_ue(0); // sps_max_latency_increase_plus1[i]
    }
    w.push_ue(0); // log2_min_luma_coding_block_size_minus3 -> log2_min_cb_size == 3
    w.push_ue(2); // log2_diff_max_min_luma_coding_block_size
    w.push_ue(0); // log2_min_luma_transform_block_size_minus2 -> log2_min_tb_size == 2
    w.push_ue(3); // log2_diff_max_min_luma_transform_block_size
    w.push_ue(3); // max_transform_hierarchy_depth_inter
    w.push_ue(3); // max_transform_hierarchy_depth_intra
    w.push_bit(knobs.scaling_list_enabled_flag as u8);
    w.push_bit(1); // amp_enabled_flag
    w.push_bit(0); // sample_adaptive_offset_enabled_flag
    w.push_bit(knobs.pcm_enabled_flag as u8);
    w.push_ue(knobs.num_short_term_ref_pic_sets);
    w.push_bit(knobs.long_term_ref_pics_present_flag as u8);
    w.push_bit(0); // sps_temporal_mvp_enabled_flag
    w.push_bit(1); // strong_intra_smoothing_enabled_flag

    w.into_bytes()
}

fn valid_sps_rbsp() -> Vec<u8> {
    sps_rbsp(&SpsKnobs::default())
}

#[test]
fn parses_every_field_correctly() {
    let sps = HevcSps::parse(&valid_sps_rbsp()).expect("valid SPS parses");
    assert_eq!(sps.general_profile_idc, 1);
    assert_eq!(sps.pic_width_in_luma_samples, 64);
    assert_eq!(sps.pic_height_in_luma_samples, 48);
    assert_eq!(sps.log2_max_pic_order_cnt_lsb, 8);
    assert_eq!(sps.max_dec_pic_buffering, 2);
    assert_eq!(sps.log2_min_cb_size, 3);
    assert_eq!(sps.log2_diff_max_min_cb_size, 2);
    assert_eq!(sps.log2_min_tb_size, 2);
    assert_eq!(sps.log2_diff_max_min_tb_size, 3);
    assert_eq!(sps.max_transform_hierarchy_depth_inter, 3);
    assert_eq!(sps.max_transform_hierarchy_depth_intra, 3);
    assert!(sps.amp_enabled_flag);
    assert!(!sps.sample_adaptive_offset_enabled_flag);
    assert!(!sps.sps_temporal_mvp_enabled_flag);
    assert!(sps.strong_intra_smoothing_enabled_flag);
}

#[test]
fn rejects_multiple_sub_layers() {
    let knobs = SpsKnobs {
        sps_max_sub_layers_minus1: 1,
        ..SpsKnobs::default()
    };
    assert_eq!(
        HevcSps::parse(&sps_rbsp(&knobs)),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_non_420_chroma_format() {
    let knobs = SpsKnobs {
        chroma_format_idc: 0,
        ..SpsKnobs::default()
    };
    assert_eq!(
        HevcSps::parse(&sps_rbsp(&knobs)),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_scaling_list_enabled() {
    let knobs = SpsKnobs {
        scaling_list_enabled_flag: 1,
        ..SpsKnobs::default()
    };
    assert_eq!(
        HevcSps::parse(&sps_rbsp(&knobs)),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_pcm_enabled() {
    let knobs = SpsKnobs {
        pcm_enabled_flag: 1,
        ..SpsKnobs::default()
    };
    assert_eq!(
        HevcSps::parse(&sps_rbsp(&knobs)),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_sps_level_short_term_rps_list() {
    let knobs = SpsKnobs {
        num_short_term_ref_pic_sets: 1,
        ..SpsKnobs::default()
    };
    assert_eq!(
        HevcSps::parse(&sps_rbsp(&knobs)),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_long_term_ref_pics_present() {
    let knobs = SpsKnobs {
        long_term_ref_pics_present_flag: 1,
        ..SpsKnobs::default()
    };
    assert_eq!(
        HevcSps::parse(&sps_rbsp(&knobs)),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_truncated_data() {
    assert_eq!(HevcSps::parse(&[]), Err(DecodeError::InvalidInput));
    // sps_video_parameter_set_id (4 bits) + sps_max_sub_layers_minus1 (3 bits) == 0 +
    // sps_temporal_id_nesting_flag (1 bit) exhausts this single all-zero byte exactly —
    // profile_tier_level() then needs many more bits than remain.
    assert_eq!(HevcSps::parse(&[0x00]), Err(DecodeError::InvalidInput));
}
