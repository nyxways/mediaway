#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

/// Minimal MSB-first bit packer used only to build test macroblock-layer bitstreams;
/// mirrors the bit order [`BitReader`] expects (same helper as the sibling `*_tests.rs`
/// files).
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

fn test_pps() -> Pps {
    Pps {
        pic_parameter_set_id: 0,
        seq_parameter_set_id: 0,
        entropy_coding_mode: false,
        num_ref_idx_l0_default_active: 1,
        num_ref_idx_l1_default_active: 1,
        pic_init_qp: 26,
        chroma_qp_index_offset: 0,
        deblocking_filter_control_present: false,
        constrained_intra_pred: false,
    }
}

#[test]
fn write_block_and_read_top_left_corner_round_trip() {
    let mut plane = vec![0u8; 8 * 8];
    let block: [u8; 16] = [
        1, 2, 3, 4, //
        5, 6, 7, 8, //
        9, 10, 11, 12, //
        13, 14, 15, 16,
    ];
    write_block::<4>(&mut plane, 8, 2, 2, &block);
    assert_eq!(plane[2 * 8 + 2], 1);
    assert_eq!(plane[2 * 8 + 5], 4);
    assert_eq!(plane[5 * 8 + 2], 13);

    let top = read_top::<4>(&plane, 8, 2, 2);
    assert_eq!(top, [0, 0, 0, 0]); // row above the block was never written (still 0).
    let corner = read_corner(&plane, 8, 2, 2);
    assert_eq!(corner, 0);

    let top_of_next_row = read_top::<4>(&plane, 8, 2, 3);
    assert_eq!(top_of_next_row, [1, 2, 3, 4]); // row 2 of the block, now "above" row 3.
}

#[test]
fn add_residual_4x4_clips_to_0_and_255() {
    let mut plane = vec![10u8; 4 * 4];
    plane[0] = 250;
    let mut residual = [0i32; 16];
    residual[0] = 100; // 250 + 100 clips to 255
    residual[1] = -20; // 10 - 20 clips to 0
    add_residual_4x4(&mut plane, 4, 0, 0, &residual);
    assert_eq!(plane[0], 255);
    assert_eq!(plane[1], 0);
}

#[test]
fn luma_nc_returns_zero_when_no_neighbors_available() {
    let ctx = McbContext::new(1, 1);
    assert_eq!(luma_nc(&ctx, 0, 0), 0);
}

#[test]
fn luma_nc_averages_left_and_top_neighbor_counts() {
    // 2x2 macroblock grid; mb_addr=3 (bottom-right) has both a left (mb 2) and top (mb 1)
    // neighbor for luma4x4BlkIdx=0 (top-left 4x4 block), whose relevant neighbor blocks are
    // blk 5 (rightmost column, same row) of mb 2, and blk 10 (bottom row, same column) of
    // mb 1.
    let mut ctx = McbContext::new(4, 2);
    ctx.luma_nz[2][xy_to_blk(3, 0)] = 4; // left neighbor's right-column block
    ctx.luma_nz[1][xy_to_blk(0, 3)] = 6; // top neighbor's bottom-row block
    assert_eq!(luma_nc(&ctx, 3, 0), (4 + 6 + 1) / 2);
}

#[test]
fn luma_nc_treats_pcm_neighbor_as_16() {
    let mut ctx = McbContext::new(2, 2);
    ctx.is_pcm[0] = true; // top-left neighbor of mb 1's blk 0 (left) is mb 0, all I_PCM.
    assert_eq!(luma_nc(&ctx, 1, 0), 16);
}

#[test]
fn decode_macroblock_reconstructs_i_pcm_samples_verbatim() {
    let mut writer = BitWriter::new();
    writer.write_ue(25); // mb_type = I_PCM (9 bits: "000011010")
    // `finish()` zero-pads to the next byte boundary (16 bits / 2 bytes here); those pad
    // bits are exactly what `align_to_byte()` consumes before the raw PCM samples begin,
    // so appending `luma` right after `bytes` lands on the correct byte offset.
    let mut bytes = writer.finish();

    let luma: Vec<u8> = (0..256).map(|i| (i % 256) as u8).collect();
    let chroma_u = vec![64u8; 64];
    let chroma_v = vec![192u8; 64];
    bytes.extend_from_slice(&luma);
    bytes.extend_from_slice(&chroma_u);
    bytes.extend_from_slice(&chroma_v);

    let mut reader = BitReader::new(&bytes);
    let pps = test_pps();
    let mut picture = Picture::new(1, 1);
    let mut ctx = McbContext::new(1, 1);
    let mut qp_prev = 26;

    decode_macroblock(&mut reader, &pps, 0, &mut qp_prev, &mut picture, &mut ctx).unwrap();

    assert_eq!(picture.y, luma);
    assert_eq!(picture.u, chroma_u);
    assert_eq!(picture.v, chroma_v);
    assert_eq!(qp_prev, 0); // I_PCM resets QPY,prev to 0 (ITU-T H.264 § 7.4.5).
    assert!(ctx.is_pcm[0]);
}
