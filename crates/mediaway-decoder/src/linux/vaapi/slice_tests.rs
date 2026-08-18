#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

/// Minimal bit-level writer producing a slice-header-shaped RBSP for round-trip tests
/// (mirrors `sps_tests.rs` / `pps_tests.rs`'s writers; kept file-local per this workspace's
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

    fn push_se(&mut self, value: i32) {
        #[allow(
            clippy::cast_sign_loss,
            reason = "magnitude fits u32 for the small test values used here"
        )]
        let code = if value > 0 {
            2 * value as u32 - 1
        } else {
            (-value) as u32 * 2
        };
        self.push_ue(code);
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

fn test_sps() -> Sps {
    Sps {
        profile_idc: 66,
        log2_max_frame_num_minus4: 4,
        pic_order_cnt_type: 0,
        log2_max_pic_order_cnt_lsb_minus4: 4,
        max_num_ref_frames: 2,
        gaps_in_frame_num_value_allowed_flag: false,
        pic_width_in_mbs_minus1: 9,
        pic_height_in_map_units_minus1: 8,
        direct_8x8_inference_flag: true,
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::fn_params_excessive_bools,
    reason = "each bool names one independent ITU-T H.264 PPS syntax element under test — \
              mirrors Pps's own struct_excessive_bools allow in pps.rs"
)]
fn test_pps(
    pic_order_present_flag: bool,
    redundant_pic_cnt_present_flag: bool,
    deblocking_filter_control_present_flag: bool,
    weighted_pred_flag: bool,
    entropy_coding_mode_flag: bool,
    num_ref_idx_l0_default_active: u32,
) -> Pps {
    Pps {
        pic_parameter_set_id: 0,
        entropy_coding_mode_flag,
        pic_order_present_flag,
        num_ref_idx_l0_default_active,
        weighted_pred_flag,
        pic_init_qp_minus26: 0,
        pic_init_qs_minus26: 0,
        chroma_qp_index_offset: 0,
        second_chroma_qp_index_offset: 0,
        deblocking_filter_control_present_flag,
        constrained_intra_pred_flag: false,
        redundant_pic_cnt_present_flag,
    }
}

/// Build an I-slice RBSP (mirrors this crate's original IDR-only test shape, extended with an
/// explicit `is_idr` so non-IDR I slices can be exercised too).
#[allow(clippy::too_many_arguments)]
fn slice_rbsp(
    sps: &Sps,
    pps: &Pps,
    nal_ref_idc: u8,
    is_idr: bool,
    first_mb_in_slice: u32,
    slice_type: u32,
    frame_num: u32,
    poc_lsb: u32,
    redundant_pic_cnt: u32,
    disable_deblocking_filter_idc: u32,
) -> Vec<u8> {
    let mut w = BitWriter::default();
    w.push_ue(first_mb_in_slice);
    w.push_ue(slice_type);
    w.push_ue(pps.pic_parameter_set_id);
    w.push_bits(frame_num, sps.log2_max_frame_num_minus4 + 4);
    if is_idr {
        w.push_ue(0); // idr_pic_id
    }
    w.push_bits(poc_lsb, sps.log2_max_pic_order_cnt_lsb_minus4 + 4);
    if pps.pic_order_present_flag {
        w.push_se(0); // delta_pic_order_cnt_bottom
    }
    if pps.redundant_pic_cnt_present_flag {
        w.push_ue(redundant_pic_cnt);
    }
    if nal_ref_idc != 0 {
        // IDR form: no_output_of_prior_pics_flag then long_term_reference_flag (2 bits).
        // Non-IDR form: adaptive_ref_pic_marking_mode_flag alone (1 bit) — both start with a
        // bit that is always 0 in these tests, so the leading push_bit is shared.
        w.push_bit(0);
        if is_idr {
            w.push_bit(0); // long_term_reference_flag
        }
    }
    w.push_se(0); // slice_qp_delta
    if pps.deblocking_filter_control_present_flag {
        w.push_ue(disable_deblocking_filter_idc);
        if disable_deblocking_filter_idc != 1 {
            w.push_se(1); // slice_alpha_c0_offset_div2
            w.push_se(-1); // slice_beta_offset_div2
        }
    }
    w.push_bit(1); // rbsp_trailing_bits() stop bit
    w.into_bytes()
}

/// Build a P-slice RBSP (single-forward-reference scope — see `slice.rs`'s module doc).
#[allow(clippy::too_many_arguments)]
fn p_slice_rbsp(
    sps: &Sps,
    pps: &Pps,
    nal_ref_idc: u8,
    frame_num: u32,
    poc_lsb: u32,
    num_ref_idx_active_override_flag: bool,
    num_ref_idx_l0_active_minus1: u32,
    ref_pic_list_modification_flag_l0: bool,
    adaptive_ref_pic_marking_mode_flag: bool,
) -> Vec<u8> {
    let mut w = BitWriter::default();
    w.push_ue(0); // first_mb_in_slice
    w.push_ue(0); // slice_type = P
    w.push_ue(pps.pic_parameter_set_id);
    w.push_bits(frame_num, sps.log2_max_frame_num_minus4 + 4);
    // is_idr == false for every P slice (an IDR access unit is always I-only).
    w.push_bits(poc_lsb, sps.log2_max_pic_order_cnt_lsb_minus4 + 4);
    if pps.pic_order_present_flag {
        w.push_se(0); // delta_pic_order_cnt_bottom
    }
    if pps.redundant_pic_cnt_present_flag {
        w.push_ue(0); // redundant_pic_cnt
    }
    w.push_bit(u8::from(num_ref_idx_active_override_flag));
    if num_ref_idx_active_override_flag {
        w.push_ue(num_ref_idx_l0_active_minus1);
    }
    w.push_bit(u8::from(ref_pic_list_modification_flag_l0));
    if ref_pic_list_modification_flag_l0 {
        w.push_ue(3); // modification_of_pic_nums_idc terminator: no real ops needed for tests
    }
    if nal_ref_idc != 0 {
        w.push_bit(u8::from(adaptive_ref_pic_marking_mode_flag));
    }
    w.push_se(0); // slice_qp_delta
    w.push_bit(1); // rbsp_trailing_bits() stop bit
    w.into_bytes()
}

#[test]
fn parses_idr_i_slice_and_computes_exact_bit_offset() {
    let sps = test_sps();
    let pps = test_pps(false, false, false, false, false, 1);
    let rbsp = slice_rbsp(&sps, &pps, 1, true, 0, 2, 3, 5, 0, 0);
    let header = SliceHeader::parse(&rbsp, 1, true, &sps, &pps).expect("valid IDR I slice parses");
    assert_eq!(header.first_mb_in_slice, 0);
    assert_eq!(header.slice_type, 2);
    assert_eq!(header.frame_num, 3);
    assert!(header.is_idr);
    assert_eq!(header.pic_order_cnt_lsb, 5);
    assert_eq!(header.num_ref_idx_l0_active, 0);
    assert_eq!(header.disable_deblocking_filter_idc, 0);
    assert_eq!(header.slice_alpha_c0_offset_div2, 0);
    assert_eq!(header.slice_beta_offset_div2, 0);
    // 1 (first_mb ue(0)) + 3 (slice_type ue(2)) + 1 (pps_id ue(0)) + 8 (frame_num, fixed) +
    // 1 (idr_pic_id ue(0)) + 8 (poc_lsb, fixed) + 2 (dec_ref_pic_marking IDR form) +
    // 1 (slice_qp_delta se(0)) = 25. Callers add 8 for the NAL header byte to get
    // `slice_data_bit_offset` — see `SliceHeader::bits_consumed` doc comment.
    assert_eq!(header.bits_consumed, 25);
}

#[test]
fn slice_type_seven_all_i_reduces_to_two() {
    let sps = test_sps();
    let pps = test_pps(false, false, false, false, false, 1);
    let rbsp = slice_rbsp(&sps, &pps, 1, true, 0, 7, 0, 0, 0, 0);
    let header = SliceHeader::parse(&rbsp, 1, true, &sps, &pps).expect("slice_type 7 is also I");
    assert_eq!(header.slice_type, 2);
}

#[test]
fn deblocking_control_present_and_enabled_reads_alpha_beta_offsets() {
    let sps = test_sps();
    let pps = test_pps(false, false, true, false, false, 1);
    let rbsp = slice_rbsp(&sps, &pps, 1, true, 0, 2, 0, 0, 0, 0);
    let header = SliceHeader::parse(&rbsp, 1, true, &sps, &pps).expect("valid slice parses");
    assert_eq!(header.disable_deblocking_filter_idc, 0);
    assert_eq!(header.slice_alpha_c0_offset_div2, 1);
    assert_eq!(header.slice_beta_offset_div2, -1);
}

#[test]
fn deblocking_disabled_idc_one_skips_alpha_beta_offsets() {
    let sps = test_sps();
    let pps = test_pps(false, false, true, false, false, 1);
    let rbsp = slice_rbsp(&sps, &pps, 1, true, 0, 2, 0, 0, 0, 1);
    let header = SliceHeader::parse(&rbsp, 1, true, &sps, &pps).expect("valid slice parses");
    assert_eq!(header.disable_deblocking_filter_idc, 1);
    assert_eq!(header.slice_alpha_c0_offset_div2, 0);
    assert_eq!(header.slice_beta_offset_div2, 0);
}

#[test]
fn rejects_multiple_slices_per_picture() {
    let sps = test_sps();
    let pps = test_pps(false, false, false, false, false, 1);
    let rbsp = slice_rbsp(&sps, &pps, 1, true, 5, 2, 0, 0, 0, 0);
    assert_eq!(
        SliceHeader::parse(&rbsp, 1, true, &sps, &pps),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_non_i_non_p_slice_types() {
    let sps = test_sps();
    let pps = test_pps(false, false, false, false, false, 1);
    let rbsp = slice_rbsp(&sps, &pps, 1, true, 0, 1, 0, 0, 0, 0); // B slice
    assert_eq!(
        SliceHeader::parse(&rbsp, 1, true, &sps, &pps),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_nonzero_redundant_pic_cnt() {
    let sps = test_sps();
    let pps = test_pps(false, true, false, false, false, 1);
    let rbsp = slice_rbsp(&sps, &pps, 1, true, 0, 2, 0, 0, 2, 0);
    assert_eq!(
        SliceHeader::parse(&rbsp, 1, true, &sps, &pps),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn truncated_slice_header_is_invalid_input() {
    let sps = test_sps();
    let pps = test_pps(false, false, false, false, false, 1);
    let rbsp: [u8; 0] = [];
    assert_eq!(
        SliceHeader::parse(&rbsp, 1, true, &sps, &pps),
        Err(DecodeError::InvalidInput)
    );
}

#[test]
fn parses_non_idr_i_slice() {
    let sps = test_sps();
    let pps = test_pps(false, false, false, false, false, 1);
    let rbsp = slice_rbsp(&sps, &pps, 1, false, 0, 2, 1, 0, 0, 0);
    let header = SliceHeader::parse(&rbsp, 1, false, &sps, &pps)
        .expect("non-IDR I slice with sliding-window marking parses");
    assert!(!header.is_idr);
    assert_eq!(header.frame_num, 1);
}

#[test]
fn parses_p_slice_with_default_num_ref_idx_l0_active() {
    let sps = test_sps();
    let pps = test_pps(false, false, false, false, false, 1);
    let rbsp = p_slice_rbsp(&sps, &pps, 1, 1, 2, false, 0, false, false);
    let header = SliceHeader::parse(&rbsp, 1, false, &sps, &pps)
        .expect("single-forward-reference P slice parses");
    assert_eq!(header.slice_type, 0);
    assert_eq!(header.frame_num, 1);
    assert_eq!(header.pic_order_cnt_lsb, 2);
    assert_eq!(header.num_ref_idx_l0_active, 1); // pps default
}

#[test]
fn parses_p_slice_with_overridden_num_ref_idx_l0_active_one() {
    let sps = test_sps();
    let pps = test_pps(false, false, false, false, false, 1);
    let rbsp = p_slice_rbsp(&sps, &pps, 1, 1, 0, true, 0, false, false);
    let header = SliceHeader::parse(&rbsp, 1, false, &sps, &pps)
        .expect("num_ref_idx_l0_active overridden to exactly 1 is still in scope");
    assert_eq!(header.num_ref_idx_l0_active, 1);
}

#[test]
fn rejects_p_slice_num_ref_idx_l0_active_other_than_one() {
    let sps = test_sps();
    let pps = test_pps(false, false, false, false, false, 1);
    let rbsp = p_slice_rbsp(&sps, &pps, 1, 1, 0, true, 1, false, false); // override -> 2 active
    assert_eq!(
        SliceHeader::parse(&rbsp, 1, false, &sps, &pps),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_p_slice_ref_pic_list_modification() {
    let sps = test_sps();
    let pps = test_pps(false, false, false, false, false, 1);
    let rbsp = p_slice_rbsp(&sps, &pps, 1, 1, 0, false, 0, true, false);
    assert_eq!(
        SliceHeader::parse(&rbsp, 1, false, &sps, &pps),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_p_slice_adaptive_ref_pic_marking() {
    let sps = test_sps();
    let pps = test_pps(false, false, false, false, false, 1);
    let rbsp = p_slice_rbsp(&sps, &pps, 1, 1, 0, false, 0, false, true);
    assert_eq!(
        SliceHeader::parse(&rbsp, 1, false, &sps, &pps),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_p_slice_when_pps_weighted_pred_flag_set() {
    let sps = test_sps();
    let pps = test_pps(false, false, false, true, false, 1);
    let rbsp = p_slice_rbsp(&sps, &pps, 1, 1, 0, false, 0, false, false);
    assert_eq!(
        SliceHeader::parse(&rbsp, 1, false, &sps, &pps),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_p_slice_when_pps_entropy_coding_mode_flag_set() {
    let sps = test_sps();
    let pps = test_pps(false, false, false, false, true, 1);
    let rbsp = p_slice_rbsp(&sps, &pps, 1, 1, 0, false, 0, false, false);
    assert_eq!(
        SliceHeader::parse(&rbsp, 1, false, &sps, &pps),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn i_slice_ignores_pps_weighted_pred_flag() {
    // weighted_pred_flag / entropy_coding_mode_flag only gate P-slice-specific syntax
    // (pred_weight_table() / cabac_init_idc) — an I slice never reads either, so a PPS setting
    // them must not affect I-slice parsing.
    let sps = test_sps();
    let pps = test_pps(false, false, false, true, true, 1);
    let rbsp = slice_rbsp(&sps, &pps, 1, true, 0, 2, 0, 0, 0, 0);
    assert!(SliceHeader::parse(&rbsp, 1, true, &sps, &pps).is_ok());
}
