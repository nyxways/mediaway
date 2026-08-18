#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

/// Minimal bit-level writer producing a PPS-shaped RBSP for round-trip tests (mirrors this
/// crate's H.264 `pps_tests.rs`'s writer; kept file-local per this workspace's sibling-test
/// convention).
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

/// Every knob a valid, minimal, this-crate-acceptable PPS RBSP can vary — defaults produce the
/// same shape [`valid_pps_rbsp`] builds. Field names mirror the real ITU-T H.265 § 7.3.2.3.1
/// syntax elements, including this ADR's own new PPS-parsing tail (`hevc_pps.rs`'s module doc).
#[allow(
    clippy::struct_field_names,
    reason = "every field is the real ITU-T H.265 § 7.3.2.3.1 syntax element name (all of which \
              happen to end in _flag) — renaming to avoid the shared suffix would obscure the \
              1:1 spec mapping this test module relies on for review"
)]
struct PpsKnobs {
    tiles_enabled_flag: u8,
    entropy_coding_sync_enabled_flag: u8,
    deblocking_filter_control_present_flag: u8,
    pps_scaling_list_data_present_flag: u8,
    lists_modification_present_flag: u8,
    slice_segment_header_extension_present_flag: u8,
    pps_extension_present_flag: u8,
}

impl Default for PpsKnobs {
    fn default() -> Self {
        Self {
            tiles_enabled_flag: 0,
            entropy_coding_sync_enabled_flag: 0,
            deblocking_filter_control_present_flag: 0,
            pps_scaling_list_data_present_flag: 0,
            lists_modification_present_flag: 1,
            slice_segment_header_extension_present_flag: 0,
            pps_extension_present_flag: 0,
        }
    }
}

/// Builds a real, spec-ordered PPS RBSP (ITU-T H.265 § 7.3.2.3.1) with `knobs`.
fn pps_rbsp(knobs: &PpsKnobs) -> Vec<u8> {
    let mut w = BitWriter::default();
    w.push_ue(0); // pps_pic_parameter_set_id
    w.push_ue(0); // pps_seq_parameter_set_id
    w.push_bit(0); // dependent_slice_segments_enabled_flag
    w.push_bit(1); // output_flag_present_flag
    w.push_bits(2, 3); // num_extra_slice_header_bits
    w.push_bit(0); // sign_data_hiding_enabled_flag
    w.push_bit(1); // cabac_init_present_flag
    w.push_ue(0); // num_ref_idx_l0_default_active_minus1 -> 1
    w.push_ue(0); // num_ref_idx_l1_default_active_minus1 -> 1
    w.push_se(-3); // init_qp_minus26 -> init_qp == 23
    w.push_bit(1); // constrained_intra_pred_flag
    w.push_bit(0); // transform_skip_enabled_flag
    w.push_bit(0); // cu_qp_delta_enabled_flag (diff_cu_qp_delta_depth not read)
    w.push_se(1); // pps_cb_qp_offset
    w.push_se(-1); // pps_cr_qp_offset
    w.push_bit(1); // pps_slice_chroma_qp_offsets_present_flag
    w.push_bit(0); // weighted_pred_flag
    w.push_bit(0); // weighted_bipred_flag
    w.push_bit(0); // transquant_bypass_enabled_flag
    w.push_bit(knobs.tiles_enabled_flag);
    w.push_bit(knobs.entropy_coding_sync_enabled_flag);
    // tiles_enabled_flag == 0 in every case this function's callers actually parse
    // successfully, so no tile-column/row syntax is written here.
    w.push_bit(1); // pps_loop_filter_across_slices_enabled_flag
    w.push_bit(knobs.deblocking_filter_control_present_flag);
    // deblocking_filter_control_present_flag == 0 in every case this function's callers
    // actually parse successfully, so no deblocking-override syntax is written here.
    w.push_bit(knobs.pps_scaling_list_data_present_flag);
    w.push_bit(knobs.lists_modification_present_flag);
    w.push_ue(1); // log2_parallel_merge_level_minus2
    w.push_bit(knobs.slice_segment_header_extension_present_flag);
    w.push_bit(knobs.pps_extension_present_flag);

    w.into_bytes()
}

fn valid_pps_rbsp() -> Vec<u8> {
    pps_rbsp(&PpsKnobs::default())
}

#[test]
fn parses_every_field_correctly() {
    let pps = HevcPps::parse(&valid_pps_rbsp()).expect("valid PPS parses");
    assert_eq!(pps.pps_pic_parameter_set_id, 0);
    assert!(!pps.dependent_slice_segments_enabled_flag);
    assert!(pps.output_flag_present_flag);
    assert_eq!(pps.num_extra_slice_header_bits, 2);
    assert!(!pps.sign_data_hiding_enabled_flag);
    assert!(pps.cabac_init_present_flag);
    assert_eq!(pps.num_ref_idx_l0_default_active, 1);
    assert_eq!(pps.num_ref_idx_l1_default_active, 1);
    assert_eq!(pps.init_qp, 23);
    assert!(pps.constrained_intra_pred_flag);
    assert!(!pps.transform_skip_enabled_flag);
    assert!(!pps.cu_qp_delta_enabled_flag);
    assert_eq!(pps.diff_cu_qp_delta_depth, 0);
    assert_eq!(pps.pps_cb_qp_offset, 1);
    assert_eq!(pps.pps_cr_qp_offset, -1);
    assert!(pps.pps_slice_chroma_qp_offsets_present_flag);
    assert!(!pps.weighted_pred_flag);
    assert!(!pps.weighted_bipred_flag);
    assert!(!pps.transquant_bypass_enabled_flag);
    assert!(pps.pps_loop_filter_across_slices_enabled_flag);
    assert!(pps.lists_modification_present_flag);
    assert_eq!(pps.log2_parallel_merge_level_minus2, 1);
}

#[test]
fn rejects_tiles_enabled() {
    let knobs = PpsKnobs {
        tiles_enabled_flag: 1,
        ..PpsKnobs::default()
    };
    assert_eq!(
        HevcPps::parse(&pps_rbsp(&knobs)),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_entropy_coding_sync_wpp() {
    let knobs = PpsKnobs {
        entropy_coding_sync_enabled_flag: 1,
        ..PpsKnobs::default()
    };
    assert_eq!(
        HevcPps::parse(&pps_rbsp(&knobs)),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_deblocking_filter_control_present() {
    let knobs = PpsKnobs {
        deblocking_filter_control_present_flag: 1,
        ..PpsKnobs::default()
    };
    assert_eq!(
        HevcPps::parse(&pps_rbsp(&knobs)),
        Err(DecodeError::Unsupported)
    );
}

/// This ADR's own new PPS-parsing tail — rejection case 1 of 3 (`hevc_pps.rs`'s module doc).
#[test]
fn rejects_scaling_list_data_present() {
    let knobs = PpsKnobs {
        pps_scaling_list_data_present_flag: 1,
        ..PpsKnobs::default()
    };
    assert_eq!(
        HevcPps::parse(&pps_rbsp(&knobs)),
        Err(DecodeError::Unsupported)
    );
}

/// This ADR's own new PPS-parsing tail — rejection case 2 of 3.
#[test]
fn rejects_slice_segment_header_extension_present() {
    let knobs = PpsKnobs {
        slice_segment_header_extension_present_flag: 1,
        ..PpsKnobs::default()
    };
    assert_eq!(
        HevcPps::parse(&pps_rbsp(&knobs)),
        Err(DecodeError::Unsupported)
    );
}

/// This ADR's own new PPS-parsing tail — rejection case 3 of 3.
#[test]
fn rejects_pps_extension_present() {
    let knobs = PpsKnobs {
        pps_extension_present_flag: 1,
        ..PpsKnobs::default()
    };
    assert_eq!(
        HevcPps::parse(&pps_rbsp(&knobs)),
        Err(DecodeError::Unsupported)
    );
}

/// `lists_modification_present_flag` is retained (not just parsed-and-discarded) — this ADR's
/// own finding that it is a real value the driver-facing struct must carry honestly (see
/// `hevc_pps.rs`'s field doc).
#[test]
fn retains_lists_modification_present_flag_when_unset() {
    let knobs = PpsKnobs {
        lists_modification_present_flag: 0,
        ..PpsKnobs::default()
    };
    let pps = HevcPps::parse(&pps_rbsp(&knobs)).expect("valid PPS parses");
    assert!(!pps.lists_modification_present_flag);
}

#[test]
fn rejects_truncated_data() {
    assert_eq!(HevcPps::parse(&[]), Err(DecodeError::InvalidInput));
}
