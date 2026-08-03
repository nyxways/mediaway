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
        gaps_in_frame_num_value_allowed_flag: false,
        pic_width_in_mbs_minus1: 9,
        pic_height_in_map_units_minus1: 8,
        direct_8x8_inference_flag: true,
    }
}

fn test_pps(
    pic_order_present_flag: bool,
    redundant_pic_cnt_present_flag: bool,
    deblocking_filter_control_present_flag: bool,
) -> Pps {
    Pps {
        pic_parameter_set_id: 0,
        entropy_coding_mode_flag: false,
        pic_order_present_flag,
        pic_init_qp_minus26: 0,
        pic_init_qs_minus26: 0,
        chroma_qp_index_offset: 0,
        second_chroma_qp_index_offset: 0,
        deblocking_filter_control_present_flag,
        constrained_intra_pred_flag: false,
        redundant_pic_cnt_present_flag,
    }
}

#[allow(clippy::too_many_arguments)]
fn slice_rbsp(
    sps: &Sps,
    pps: &Pps,
    nal_ref_idc: u8,
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
    w.push_ue(0); // idr_pic_id
    w.push_bits(poc_lsb, sps.log2_max_pic_order_cnt_lsb_minus4 + 4);
    if pps.pic_order_present_flag {
        w.push_se(0); // delta_pic_order_cnt_bottom
    }
    if pps.redundant_pic_cnt_present_flag {
        w.push_ue(redundant_pic_cnt);
    }
    if nal_ref_idc != 0 {
        w.push_bit(0); // no_output_of_prior_pics_flag
        w.push_bit(0); // long_term_reference_flag
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

#[test]
fn parses_idr_i_slice_and_computes_exact_bit_offset() {
    let sps = test_sps();
    let pps = test_pps(false, false, false);
    let rbsp = slice_rbsp(&sps, &pps, 1, 0, 2, 3, 5, 0, 0);
    let header = SliceHeader::parse(&rbsp, 1, &sps, &pps).expect("valid IDR I slice parses");
    assert_eq!(header.first_mb_in_slice, 0);
    assert_eq!(header.slice_type, 2);
    assert_eq!(header.frame_num, 3);
    assert_eq!(header.pic_order_cnt_lsb, 5);
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
    let pps = test_pps(false, false, false);
    let rbsp = slice_rbsp(&sps, &pps, 1, 0, 7, 0, 0, 0, 0);
    let header = SliceHeader::parse(&rbsp, 1, &sps, &pps).expect("slice_type 7 is also I");
    assert_eq!(header.slice_type, 2);
}

#[test]
fn deblocking_control_present_and_enabled_reads_alpha_beta_offsets() {
    let sps = test_sps();
    let pps = test_pps(false, false, true);
    let rbsp = slice_rbsp(&sps, &pps, 1, 0, 2, 0, 0, 0, 0);
    let header = SliceHeader::parse(&rbsp, 1, &sps, &pps).expect("valid slice parses");
    assert_eq!(header.disable_deblocking_filter_idc, 0);
    assert_eq!(header.slice_alpha_c0_offset_div2, 1);
    assert_eq!(header.slice_beta_offset_div2, -1);
}

#[test]
fn deblocking_disabled_idc_one_skips_alpha_beta_offsets() {
    let sps = test_sps();
    let pps = test_pps(false, false, true);
    let rbsp = slice_rbsp(&sps, &pps, 1, 0, 2, 0, 0, 0, 1);
    let header = SliceHeader::parse(&rbsp, 1, &sps, &pps).expect("valid slice parses");
    assert_eq!(header.disable_deblocking_filter_idc, 1);
    assert_eq!(header.slice_alpha_c0_offset_div2, 0);
    assert_eq!(header.slice_beta_offset_div2, 0);
}

#[test]
fn rejects_multiple_slices_per_picture() {
    let sps = test_sps();
    let pps = test_pps(false, false, false);
    let rbsp = slice_rbsp(&sps, &pps, 1, 5, 2, 0, 0, 0, 0);
    assert_eq!(
        SliceHeader::parse(&rbsp, 1, &sps, &pps),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_non_i_slice_types() {
    let sps = test_sps();
    let pps = test_pps(false, false, false);
    let rbsp = slice_rbsp(&sps, &pps, 1, 0, 0, 0, 0, 0, 0); // P slice
    assert_eq!(
        SliceHeader::parse(&rbsp, 1, &sps, &pps),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_nonzero_redundant_pic_cnt() {
    let sps = test_sps();
    let pps = test_pps(false, true, false);
    let rbsp = slice_rbsp(&sps, &pps, 1, 0, 2, 0, 0, 2, 0);
    assert_eq!(
        SliceHeader::parse(&rbsp, 1, &sps, &pps),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn truncated_slice_header_is_invalid_input() {
    let sps = test_sps();
    let pps = test_pps(false, false, false);
    let rbsp: [u8; 0] = [];
    assert_eq!(
        SliceHeader::parse(&rbsp, 1, &sps, &pps),
        Err(DecodeError::InvalidInput)
    );
}
