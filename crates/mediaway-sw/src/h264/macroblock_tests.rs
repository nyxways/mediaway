#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

/// Minimal MSB-first bit packer used only to build test bitstreams; mirrors the bit order
/// [`BitReader`] expects (same helper as the sibling `*_tests.rs` files).
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

    fn finish(mut self) -> Vec<u8> {
        while self.nbits != 0 {
            self.push_bit(0);
        }
        self.bytes
    }
}

#[test]
fn from_raw_decodes_i_nxn() {
    assert_eq!(MbType::from_raw(0).unwrap(), MbType::INxN);
}

#[test]
fn from_raw_decodes_i_pcm() {
    assert_eq!(MbType::from_raw(25).unwrap(), MbType::IPcm);
}

#[test]
fn from_raw_decodes_i16x16_first_and_last_no_ac_entries() {
    // mb_type 1: pred_mode=0 (Vertical), cbp_chroma=0, cbp_luma=0 (Table 7-11 row 1).
    assert_eq!(
        MbType::from_raw(1).unwrap(),
        MbType::I16x16 {
            pred_mode: 0,
            cbp_luma: 0,
            cbp_chroma: 0
        }
    );
    // mb_type 12: base=11 -> pred_mode=3, cbp_chroma=(11/4)%3=2, cbp_luma=0 (11/12==0).
    assert_eq!(
        MbType::from_raw(12).unwrap(),
        MbType::I16x16 {
            pred_mode: 3,
            cbp_luma: 0,
            cbp_chroma: 2
        }
    );
}

#[test]
fn from_raw_decodes_i16x16_ac_present_entries() {
    // mb_type 13: base=12 -> pred_mode=0, cbp_chroma=0, cbp_luma=15 (12/12==1).
    assert_eq!(
        MbType::from_raw(13).unwrap(),
        MbType::I16x16 {
            pred_mode: 0,
            cbp_luma: 15,
            cbp_chroma: 0
        }
    );
    // mb_type 24: base=23 -> pred_mode=3, cbp_chroma=(23/4)%3=2, cbp_luma=15.
    assert_eq!(
        MbType::from_raw(24).unwrap(),
        MbType::I16x16 {
            pred_mode: 3,
            cbp_luma: 15,
            cbp_chroma: 2
        }
    );
}

#[test]
fn from_raw_errors_above_valid_range() {
    assert_eq!(MbType::from_raw(26), Err(H264Error::InvalidMbType));
}

#[test]
fn read_intra_chroma_pred_mode_accepts_all_valid_values() {
    for value in 0u32..=3 {
        let mut writer = BitWriter::new();
        writer.write_ue(value);
        let bytes = writer.finish();
        let mut reader = BitReader::new(&bytes);
        assert_eq!(
            read_intra_chroma_pred_mode(&mut reader).unwrap(),
            u8::try_from(value).unwrap()
        );
    }
}

#[test]
fn read_intra_chroma_pred_mode_rejects_out_of_range_value() {
    let mut writer = BitWriter::new();
    writer.write_ue(4);
    let bytes = writer.finish();
    let mut reader = BitReader::new(&bytes);
    assert_eq!(
        read_intra_chroma_pred_mode(&mut reader),
        Err(H264Error::InvalidMbType)
    );
}
