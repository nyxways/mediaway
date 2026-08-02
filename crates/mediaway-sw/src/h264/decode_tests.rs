#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test modules may unwrap/panic on an assertion failure"
)]

use super::*;
use crate::h264::nal::NalUnitType;
use mediaway_common::Bytes;

/// Minimal MSB-first bit packer used only to build test slice-NAL bitstreams; mirrors the
/// bit order [`BitReader`] expects (same helper as the sibling `*_tests.rs` files).
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

/// 1 macroblock (16x16 luma / 8x8 chroma), `pic_init_qp = 28`, `pic_order_cnt_type = 0`.
fn one_mb_sps() -> Sps {
    Sps {
        profile_idc: 66,
        constraint_flags: 0,
        level_idc: 30,
        seq_parameter_set_id: 0,
        width: 16,
        height: 16,
        frame_mbs_only: true,
        chroma_format_idc: 1,
        log2_max_frame_num: 4,
        pic_order_cnt_type: 0,
        log2_max_pic_order_cnt_lsb: 4,
        pic_width_in_mbs: 1,
        pic_height_in_mbs: 1,
    }
}

fn one_mb_pps(entropy_coding_mode: bool) -> Pps {
    Pps {
        pic_parameter_set_id: 0,
        seq_parameter_set_id: 0,
        entropy_coding_mode,
        num_ref_idx_l0_default_active: 1,
        num_ref_idx_l1_default_active: 1,
        pic_init_qp: 28,
        chroma_qp_index_offset: 0,
        deblocking_filter_control_present: false,
        constrained_intra_pred: false,
    }
}

/// Write a minimal I-slice header (IDR, `pic_order_cnt_type == 0`, no deblocking-control
/// fields) matching [`one_mb_sps`]/[`one_mb_pps`], with `slice_qp_delta = 0`.
fn write_idr_i_slice_header(writer: &mut BitWriter, first_mb_in_slice: u32, slice_type: u32) {
    writer.write_ue(first_mb_in_slice);
    writer.write_ue(slice_type);
    writer.write_ue(0); // pic_parameter_set_id
    writer.write_bits(0, 4); // frame_num
    writer.write_ue(0); // idr_pic_id
    writer.write_bits(0, 4); // pic_order_cnt_lsb
    writer.push_bit(0); // no_output_of_prior_pics_flag
    writer.push_bit(0); // long_term_reference_flag
    writer.write_se(0); // slice_qp_delta
}

fn idr_nal(rbsp: Vec<u8>) -> NalUnit {
    NalUnit {
        ref_idc: 1,
        unit_type: NalUnitType::IdrSlice,
        rbsp: Bytes::from(rbsp),
    }
}

#[test]
fn decode_i_frame_reconstructs_a_solid_color_one_macroblock_picture() {
    let mut writer = BitWriter::new();
    write_idr_i_slice_header(&mut writer, 0, 2);

    // mb_type = 3 -> I_16x16, pred_mode=2 (DC), cbp_luma=0, cbp_chroma=0.
    writer.write_ue(3);
    writer.write_ue(0); // intra_chroma_pred_mode = DC
    writer.write_se(0); // mb_qp_delta -> QPy = pic_init_qp(28) + slice_qp_delta(0) = 28

    // Intra16x16DCLevel residual: nC=0 (no neighbors) -> VLC0. TotalCoeff=1,
    // TrailingOnes=0, level_prefix=6 (no trailing-ones sign bit, no level suffix) decodes
    // to level = +5 at scan position 0 (TotalZeros=0).
    writer.write_bits(0b00_0101, 6); // coeff_token(TotalCoeff=1, TrailingOnes=0), VLC0
    for _ in 0..6 {
        writer.push_bit(0); // level_prefix unary: 6 zero bits...
    }
    writer.push_bit(1); // ...then the terminating 1 bit (level_prefix = 6)
    writer.push_bit(1); // total_zeros(TotalZeros=0, TotalCoeff=1) = "1"

    let nal = idr_nal(writer.finish());
    let sps = one_mb_sps();
    let pps = one_mb_pps(false);

    let frame = decode_i_frame(&sps, &pps, &nal).unwrap();
    assert_eq!(frame.width, 16);
    assert_eq!(frame.height, 16);
    assert_eq!(frame.format, mediaway_common::PixelFormat::I420);

    let VideoFrameStorage::Cpu { data } = &frame.storage else {
        panic!("expected CPU storage");
    };
    assert_eq!(data.len(), 16 * 16 + 2 * 8 * 8);

    // DC-only Hadamard/transform input spreads evenly (see transform.rs's own tests): a
    // lone luma DC coefficient of level 5 dequantizes to 320 at QP 28, and the core
    // transform's `(x + 32) >> 6` normalization turns a uniform 320 input into a uniform
    // residual of 5 added on top of the DC-mode (128, no neighbors) prediction.
    let y = &data[0..256];
    assert!(
        y.iter().all(|&v| v == 133),
        "luma should be uniformly 133, got {y:?}"
    );

    // cbp_chroma == 0 -> no chroma residual at all; DC chroma prediction with no
    // neighbors is the flat 128 fallback.
    let u = &data[256..256 + 64];
    let v = &data[256 + 64..256 + 128];
    assert!(
        u.iter().all(|&val| val == 128),
        "chroma U should be uniformly 128, got {u:?}"
    );
    assert!(
        v.iter().all(|&val| val == 128),
        "chroma V should be uniformly 128, got {v:?}"
    );
}

#[test]
fn decode_i_frame_rejects_cabac() {
    let mut writer = BitWriter::new();
    write_idr_i_slice_header(&mut writer, 0, 2);
    let nal = idr_nal(writer.finish());
    let sps = one_mb_sps();
    let pps = one_mb_pps(true);

    assert_eq!(
        decode_i_frame(&sps, &pps, &nal),
        Err(H264Error::UnsupportedEntropyCoding)
    );
}

#[test]
fn decode_i_frame_rejects_non_4_2_0_chroma_format() {
    let mut sps = one_mb_sps();
    sps.chroma_format_idc = 2;
    let pps = one_mb_pps(false);
    let mut writer = BitWriter::new();
    write_idr_i_slice_header(&mut writer, 0, 2);
    let nal = idr_nal(writer.finish());

    assert_eq!(
        decode_i_frame(&sps, &pps, &nal),
        Err(H264Error::UnsupportedChromaFormat)
    );
}

#[test]
fn decode_i_frame_rejects_p_slice() {
    let mut writer = BitWriter::new();
    write_idr_i_slice_header(&mut writer, 0, 0); // slice_type = P
    let nal = idr_nal(writer.finish());
    let sps = one_mb_sps();
    let pps = one_mb_pps(false);

    assert_eq!(
        decode_i_frame(&sps, &pps, &nal),
        Err(H264Error::UnsupportedSliceType)
    );
}

#[test]
fn decode_i_frame_rejects_non_zero_first_mb_in_slice() {
    let mut writer = BitWriter::new();
    write_idr_i_slice_header(&mut writer, 1, 2);
    let nal = idr_nal(writer.finish());
    let sps = one_mb_sps();
    let pps = one_mb_pps(false);

    assert_eq!(
        decode_i_frame(&sps, &pps, &nal),
        Err(H264Error::MultiSliceUnsupported)
    );
}

#[test]
fn decode_i_frame_rejects_i_nxn_macroblocks() {
    let mut writer = BitWriter::new();
    write_idr_i_slice_header(&mut writer, 0, 2);
    writer.write_ue(0); // mb_type = 0 -> I_NxN
    let nal = idr_nal(writer.finish());
    let sps = one_mb_sps();
    let pps = one_mb_pps(false);

    assert_eq!(
        decode_i_frame(&sps, &pps, &nal),
        Err(H264Error::UnsupportedMbType)
    );
}
