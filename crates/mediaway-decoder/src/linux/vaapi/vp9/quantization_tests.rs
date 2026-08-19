#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap / expect"
)]

use super::*;

fn pack_bits(bits: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; bits.len().div_ceil(8)];
    for (i, &bit) in bits.iter().enumerate() {
        if bit != 0 {
            out[i / 8] |= 1 << (7 - (i % 8));
        }
    }
    out
}

#[test]
fn parse_all_zero_deltas_is_lossless_when_base_q_idx_zero() {
    // base_q_idx = 0 (8 bits), then three delta_coded flags = 0.
    let mut bits = vec![0u8; 8];
    bits.extend([0, 0, 0]);
    let data = pack_bits(&bits);
    let mut r = BitReader::new(&data);
    let params = parse(&mut r).unwrap();
    assert_eq!(params.base_q_idx, 0);
    assert!(params.lossless);
}

#[test]
fn parse_nonzero_base_q_idx_is_not_lossless() {
    // base_q_idx = 10 (0b00001010), three delta_coded flags = 0.
    let mut bits = Vec::new();
    for i in (0..8).rev() {
        bits.push((10u8 >> i) & 1);
    }
    bits.extend([0, 0, 0]);
    let data = pack_bits(&bits);
    let mut r = BitReader::new(&data);
    let params = parse(&mut r).unwrap();
    assert_eq!(params.base_q_idx, 10);
    assert!(!params.lossless);
}

#[test]
fn parse_reads_coded_delta_q_and_is_not_lossless() {
    // base_q_idx = 0, delta_q_y_dc: coded=1, s(4) = magnitude 0b0011 (3), sign 1 -> -3.
    let mut bits = vec![0u8; 8];
    bits.push(1); // delta_coded for y_dc
    bits.extend([0, 0, 1, 1]); // magnitude 3
    bits.push(1); // sign: negative
    bits.push(0); // uv_dc not coded
    bits.push(0); // uv_ac not coded
    let data = pack_bits(&bits);
    let mut r = BitReader::new(&data);
    let params = parse(&mut r).unwrap();
    assert_eq!(params.base_q_idx, 0);
    assert!(!params.lossless); // delta_q_y_dc = -3 != 0
}

#[test]
fn parse_consumes_exactly_11_bits_when_no_deltas_coded() {
    let mut bits = vec![0u8; 8];
    bits.extend([0, 0, 0]);
    bits.push(1); // trailing marker bit that must remain unread
    let data = pack_bits(&bits);
    let mut r = BitReader::new(&data);
    parse(&mut r).unwrap();
    assert_eq!(r.read_bit().unwrap(), 1);
}
