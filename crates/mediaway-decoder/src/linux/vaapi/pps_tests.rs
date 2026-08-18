#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

/// Minimal bit-level writer producing a PPS-shaped RBSP for round-trip tests (mirrors
/// `sps_tests.rs`'s writer; kept file-local per this workspace's sibling-test convention).
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

#[allow(
    clippy::too_many_arguments,
    clippy::similar_names,
    reason = "pic_init_qp_minus26 / pic_init_qs_minus26 mirror the ITU-T H.264 spec's own names"
)]
fn pps_rbsp(
    entropy_coding_mode_flag: u8,
    pic_order_present_flag: u8,
    num_slice_groups_minus1: u32,
    num_ref_idx_l0_default_active_minus1: u32,
    weighted_pred_flag: u8,
    pic_init_qp_minus26: i32,
    pic_init_qs_minus26: i32,
    chroma_qp_index_offset: i32,
    deblocking_filter_control_present_flag: u8,
    constrained_intra_pred_flag: u8,
    redundant_pic_cnt_present_flag: u8,
    extension: Option<(u8, u8, i32)>,
) -> Vec<u8> {
    let mut w = BitWriter::default();
    w.push_ue(0); // pic_parameter_set_id
    w.push_ue(0); // seq_parameter_set_id
    w.push_bit(entropy_coding_mode_flag);
    w.push_bit(pic_order_present_flag);
    w.push_ue(num_slice_groups_minus1);
    w.push_ue(num_ref_idx_l0_default_active_minus1);
    w.push_ue(0); // num_ref_idx_l1_default_active_minus1
    w.push_bit(weighted_pred_flag);
    w.push_bits(0, 2); // weighted_bipred_idc
    w.push_se(pic_init_qp_minus26);
    w.push_se(pic_init_qs_minus26);
    w.push_se(chroma_qp_index_offset);
    w.push_bit(deblocking_filter_control_present_flag);
    w.push_bit(constrained_intra_pred_flag);
    w.push_bit(redundant_pic_cnt_present_flag);
    if let Some((transform_8x8, pic_scaling, second_chroma_qp)) = extension {
        w.push_bit(transform_8x8);
        w.push_bit(pic_scaling);
        w.push_se(second_chroma_qp);
    }
    w.push_bit(1); // rbsp_trailing_bits() stop bit
    w.into_bytes()
}

#[test]
fn parses_pps_without_extension_infers_second_chroma_qp_offset() {
    let rbsp = pps_rbsp(0, 0, 0, 0, 0, 0, 0, 2, 1, 0, 0, None);
    let pps = Pps::parse(&rbsp).expect("valid PPS parses");
    assert_eq!(pps.pic_parameter_set_id, 0);
    assert!(!pps.entropy_coding_mode_flag);
    assert_eq!(pps.num_ref_idx_l0_default_active, 1); // minus1 == 0
    assert!(!pps.weighted_pred_flag);
    assert_eq!(pps.chroma_qp_index_offset, 2);
    assert_eq!(pps.second_chroma_qp_index_offset, 2); // inferred equal per spec
    assert!(pps.deblocking_filter_control_present_flag);
}

#[test]
fn parses_pps_with_trivial_extension_reads_second_chroma_qp_offset() {
    let rbsp = pps_rbsp(1, 1, 0, 2, 1, -2, -2, 2, 0, 1, 0, Some((0, 0, 3)));
    let pps = Pps::parse(&rbsp).expect("valid PPS with trivial extension parses");
    assert!(pps.entropy_coding_mode_flag);
    assert!(pps.pic_order_present_flag);
    assert_eq!(pps.num_ref_idx_l0_default_active, 3); // minus1 == 2
    assert!(pps.weighted_pred_flag);
    assert_eq!(pps.chroma_qp_index_offset, 2);
    assert_eq!(pps.second_chroma_qp_index_offset, 3);
}

#[test]
fn rejects_multiple_slice_groups() {
    let rbsp = pps_rbsp(0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, None);
    assert_eq!(Pps::parse(&rbsp), Err(DecodeError::Unsupported));
}

#[test]
fn rejects_transform_8x8_extension() {
    let rbsp = pps_rbsp(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, Some((1, 0, 0)));
    assert_eq!(Pps::parse(&rbsp), Err(DecodeError::Unsupported));
}

#[test]
fn rejects_custom_scaling_matrix_extension() {
    let rbsp = pps_rbsp(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, Some((0, 1, 0)));
    assert_eq!(Pps::parse(&rbsp), Err(DecodeError::Unsupported));
}

#[test]
fn truncated_pps_is_invalid_input() {
    let rbsp: [u8; 0] = [];
    assert_eq!(Pps::parse(&rbsp), Err(DecodeError::InvalidInput));
}
