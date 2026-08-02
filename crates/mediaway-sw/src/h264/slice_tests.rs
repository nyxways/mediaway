#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

/// Minimal MSB-first bit packer used only to build test slice-header bitstreams; mirrors
/// the bit order [`BitReader`] expects (same helper as the sibling `*_tests.rs` files).
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
            self.push_bit(value >> i);
        }
    }

    fn write_ue(&mut self, value: u32) {
        let code = value + 1;
        let len = u32::BITS - code.leading_zeros();
        for _ in 0..(len - 1) {
            self.push_bit(0);
        }
        self.write_bits(code, len);
    }

    fn write_se(&mut self, value: i32) {
        let magnitude = value.unsigned_abs();
        let code = if value <= 0 {
            magnitude * 2
        } else {
            magnitude * 2 - 1
        };
        self.write_ue(code);
    }

    fn finish(mut self) -> Vec<u8> {
        while self.nbits != 0 {
            self.push_bit(0);
        }
        self.bytes
    }
}

/// A minimal but valid baseline SPS: 1 macroblock wide/tall, frame pictures,
/// `pic_order_cnt_type == 0`. Callers override the two fields under test.
fn baseline_sps(log2_max_frame_num: u32, pic_order_cnt_type: u32) -> Sps {
    Sps {
        profile_idc: 66,
        constraint_flags: 0,
        level_idc: 30,
        seq_parameter_set_id: 0,
        width: 16,
        height: 16,
        frame_mbs_only: true,
        chroma_format_idc: 1,
        log2_max_frame_num,
        pic_order_cnt_type,
        log2_max_pic_order_cnt_lsb: 4,
        pic_width_in_mbs: 1,
        pic_height_in_mbs: 1,
    }
}

fn baseline_pps(deblocking_filter_control_present: bool) -> Pps {
    Pps {
        pic_parameter_set_id: 0,
        seq_parameter_set_id: 0,
        entropy_coding_mode: false,
        num_ref_idx_l0_default_active: 1,
        num_ref_idx_l1_default_active: 1,
        pic_init_qp: 26,
        chroma_qp_index_offset: 0,
        deblocking_filter_control_present,
        constrained_intra_pred: false,
    }
}

#[test]
fn parse_extracts_idr_i_slice_fields_and_stops_at_slice_data() {
    let sps = baseline_sps(4, 0);
    let pps = baseline_pps(false);

    let mut writer = BitWriter::new();
    writer.write_ue(0); // first_mb_in_slice
    writer.write_ue(2); // slice_type = I
    writer.write_ue(0); // pic_parameter_set_id
    writer.write_bits(5, 4); // frame_num (log2_max_frame_num = 4)
    writer.write_ue(7); // idr_pic_id (IDR NAL)
    writer.write_bits(3, 4); // pic_order_cnt_lsb (log2_max_pic_order_cnt_lsb = 4)
    writer.push_bit(0); // no_output_of_prior_pics_flag (nal_ref_idc != 0, IDR)
    writer.push_bit(1); // long_term_reference_flag
    writer.write_se(-2); // slice_qp_delta
    writer.write_bits(0b101, 3); // marker: start of slice_data()

    let bytes = writer.finish();
    let mut reader = BitReader::new(&bytes);
    let header = SliceHeader::parse(&mut reader, &sps, &pps, NalUnitType::IdrSlice, 1).unwrap();

    assert_eq!(header.first_mb_in_slice, 0);
    assert_eq!(header.slice_type, SliceType::I);
    assert_eq!(header.pic_parameter_set_id, 0);
    assert_eq!(header.frame_num, 5);
    assert_eq!(header.idr_pic_id, Some(7));
    assert_eq!(header.pic_order_cnt_lsb, Some(3));
    assert_eq!(header.slice_qp_delta, -2);
    // Reader is positioned exactly at slice_data(): the marker bits follow untouched.
    assert_eq!(reader.read_bits(3).unwrap(), 0b101);
}

#[test]
fn parse_rejects_p_slice_type() {
    let sps = baseline_sps(4, 0);
    let pps = baseline_pps(false);

    let mut writer = BitWriter::new();
    writer.write_ue(0); // first_mb_in_slice
    writer.write_ue(0); // slice_type = P
    let bytes = writer.finish();
    let mut reader = BitReader::new(&bytes);

    assert_eq!(
        SliceHeader::parse(&mut reader, &sps, &pps, NalUnitType::NonIdrSlice, 1),
        Err(H264Error::UnsupportedSliceType)
    );
}

#[test]
fn parse_rejects_non_zero_pic_order_cnt_type_before_reading_bits() {
    let sps = baseline_sps(4, 1);
    let pps = baseline_pps(false);
    let mut reader = BitReader::new(&[]); // no bits available; must fail before any read

    assert_eq!(
        SliceHeader::parse(&mut reader, &sps, &pps, NalUnitType::IdrSlice, 1),
        Err(H264Error::UnsupportedPicOrderCntType)
    );
}

#[test]
fn parse_rejects_field_coded_pictures_before_reading_bits() {
    let mut sps = baseline_sps(4, 0);
    sps.frame_mbs_only = false;
    let pps = baseline_pps(false);
    let mut reader = BitReader::new(&[]);

    assert_eq!(
        SliceHeader::parse(&mut reader, &sps, &pps, NalUnitType::IdrSlice, 1),
        Err(H264Error::UnsupportedFieldCoding)
    );
}

#[test]
fn parse_consumes_adaptive_ref_pic_marking_loop_for_non_idr_ref_slice() {
    let sps = baseline_sps(4, 0);
    let pps = baseline_pps(false);

    let mut writer = BitWriter::new();
    writer.write_ue(0); // first_mb_in_slice
    writer.write_ue(7); // slice_type = 7 -> I (7 % 5 == 2)
    writer.write_ue(0); // pic_parameter_set_id
    writer.write_bits(2, 4); // frame_num
    // No idr_pic_id: not an IDR NAL.
    writer.write_bits(1, 4); // pic_order_cnt_lsb
    writer.push_bit(1); // adaptive_ref_pic_marking_mode_flag
    writer.write_ue(1); // memory_management_control_operation = 1
    writer.write_ue(0); // difference_of_pic_nums_minus1
    writer.write_ue(0); // memory_management_control_operation = 0 (end of loop)
    writer.write_se(0); // slice_qp_delta

    let bytes = writer.finish();
    let mut reader = BitReader::new(&bytes);
    let header = SliceHeader::parse(&mut reader, &sps, &pps, NalUnitType::NonIdrSlice, 2).unwrap();

    assert_eq!(header.slice_type, SliceType::I);
    assert_eq!(header.frame_num, 2);
    assert_eq!(header.idr_pic_id, None);
    assert_eq!(header.pic_order_cnt_lsb, Some(1));
    assert_eq!(header.slice_qp_delta, 0);
}

#[test]
fn parse_consumes_deblocking_offsets_when_filter_not_disabled() {
    let sps = baseline_sps(4, 0);
    let pps = baseline_pps(true);

    let mut writer = BitWriter::new();
    writer.write_ue(0); // first_mb_in_slice
    writer.write_ue(2); // slice_type = I
    writer.write_ue(0); // pic_parameter_set_id
    writer.write_bits(0, 4); // frame_num
    writer.write_bits(0, 4); // pic_order_cnt_lsb
    // nal_ref_idc == 0 below, so no dec_ref_pic_marking().
    writer.write_se(1); // slice_qp_delta
    writer.write_ue(0); // disable_deblocking_filter_idc (0 -> offsets follow)
    writer.write_se(2); // slice_alpha_c0_offset_div2
    writer.write_se(-1); // slice_beta_offset_div2
    writer.write_bits(0b11, 2); // marker: start of slice_data()

    let bytes = writer.finish();
    let mut reader = BitReader::new(&bytes);
    let header = SliceHeader::parse(&mut reader, &sps, &pps, NalUnitType::NonIdrSlice, 0).unwrap();

    assert_eq!(header.slice_qp_delta, 1);
    assert_eq!(reader.read_bits(2).unwrap(), 0b11);
}

#[test]
fn parse_skips_deblocking_offsets_when_filter_disabled() {
    let sps = baseline_sps(4, 0);
    let pps = baseline_pps(true);

    let mut writer = BitWriter::new();
    writer.write_ue(0); // first_mb_in_slice
    writer.write_ue(2); // slice_type = I
    writer.write_ue(0); // pic_parameter_set_id
    writer.write_bits(0, 4); // frame_num
    writer.write_bits(0, 4); // pic_order_cnt_lsb
    writer.write_se(0); // slice_qp_delta
    writer.write_ue(1); // disable_deblocking_filter_idc == 1 -> no offsets follow
    writer.write_bits(0b10, 2); // marker: start of slice_data()

    let bytes = writer.finish();
    let mut reader = BitReader::new(&bytes);
    let header = SliceHeader::parse(&mut reader, &sps, &pps, NalUnitType::NonIdrSlice, 0).unwrap();

    assert_eq!(header.slice_qp_delta, 0);
    assert_eq!(reader.read_bits(2).unwrap(), 0b10);
}
