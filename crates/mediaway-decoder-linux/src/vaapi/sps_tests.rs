#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

/// Minimal bit-level writer producing an SPS-shaped RBSP for round-trip tests. Only needs to
/// cover the fields [`Sps::parse`] actually reads — anything after `direct_8x8_inference_flag`
/// is never consumed, so trailing bits/bytes need no `rbsp_trailing_bits()` shaping.
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

    /// Pack into bytes, MSB-first, zero-padding the final byte.
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

/// Build a baseline-profile (`profile_idc = 66`) SPS RBSP with the given raw field values
/// (everything through `direct_8x8_inference_flag`, then a few harmless trailing bits).
#[allow(clippy::too_many_arguments)]
fn baseline_sps_rbsp(
    log2_max_frame_num_minus4: u32,
    pic_order_cnt_type: u32,
    log2_max_pic_order_cnt_lsb_minus4: u32,
    pic_width_in_mbs_minus1: u32,
    pic_height_in_map_units_minus1: u32,
    frame_mbs_only_flag: u8,
    direct_8x8_inference_flag: u8,
) -> Vec<u8> {
    let mut out = vec![66u8, 0x00, 30]; // profile_idc, constraint flags, level_idc
    let mut w = BitWriter::default();
    w.push_ue(0); // seq_parameter_set_id
    w.push_ue(log2_max_frame_num_minus4);
    w.push_ue(pic_order_cnt_type);
    if pic_order_cnt_type == 0 {
        w.push_ue(log2_max_pic_order_cnt_lsb_minus4);
    }
    w.push_ue(0); // max_num_ref_frames
    w.push_bit(0); // gaps_in_frame_num_value_allowed_flag
    w.push_ue(pic_width_in_mbs_minus1);
    w.push_ue(pic_height_in_map_units_minus1);
    w.push_bit(frame_mbs_only_flag);
    w.push_bit(direct_8x8_inference_flag);
    w.push_bit(0); // frame_cropping_flag
    w.push_bit(0); // vui_parameters_present_flag
    w.push_bit(1); // a few harmless trailing bits (never read)
    out.extend(w.into_bytes());
    out
}

#[test]
fn parses_baseline_sps_dimensions_and_fields() {
    let rbsp = baseline_sps_rbsp(4, 0, 4, 9, 8, 1, 1);
    let sps = Sps::parse(&rbsp).expect("valid baseline SPS parses");
    assert_eq!(sps.profile_idc, 66);
    assert_eq!(sps.log2_max_frame_num_minus4, 4);
    assert_eq!(sps.pic_order_cnt_type, 0);
    assert_eq!(sps.log2_max_pic_order_cnt_lsb_minus4, 4);
    assert!(!sps.gaps_in_frame_num_value_allowed_flag);
    assert!(sps.direct_8x8_inference_flag);
    assert_eq!(sps.width(), 160); // (9 + 1) * 16
    assert_eq!(sps.height(), 144); // (8 + 1) * 16
}

#[test]
fn rejects_high_profile_idc_as_unsupported() {
    let mut rbsp = baseline_sps_rbsp(4, 0, 4, 9, 8, 1, 1);
    rbsp[0] = 100; // High profile — carries an extra SPS field block this parser does not read
    assert_eq!(Sps::parse(&rbsp), Err(DecodeError::Unsupported));
}

#[test]
fn rejects_pic_order_cnt_type_other_than_zero() {
    let rbsp = baseline_sps_rbsp(4, 2, 0, 9, 8, 1, 1);
    assert_eq!(Sps::parse(&rbsp), Err(DecodeError::Unsupported));
}

#[test]
fn rejects_interlaced_frame_mbs_only_flag_zero() {
    let rbsp = baseline_sps_rbsp(4, 0, 4, 9, 8, 0, 1);
    assert_eq!(Sps::parse(&rbsp), Err(DecodeError::Unsupported));
}

#[test]
fn truncated_sps_is_invalid_input() {
    let rbsp = [66u8, 0x00, 30]; // header only, no bitstream fields at all
    assert_eq!(Sps::parse(&rbsp), Err(DecodeError::InvalidInput));
}
