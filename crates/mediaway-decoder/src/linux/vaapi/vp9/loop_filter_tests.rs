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
fn parse_reads_level_and_sharpness_with_deltas_disabled() {
    // level=0b111111 (63), sharpness=0b101 (5), delta_enabled=0.
    let mut bits = vec![1, 1, 1, 1, 1, 1]; // level: 6 bits
    bits.extend([1, 0, 1]); // sharpness: 3 bits = 5
    bits.push(0); // delta_enabled = 0
    let data = pack_bits(&bits);
    let mut r = BitReader::new(&data);
    let params = parse(&mut r).unwrap();
    assert_eq!(params.level, 63);
    assert_eq!(params.sharpness, 5);
}

#[test]
fn parse_skips_delta_reads_when_delta_update_clear() {
    // level=0, sharpness=0, delta_enabled=1, delta_update=0.
    let mut bits = vec![0; 6]; // level
    bits.extend([0; 3]); // sharpness
    bits.push(1); // delta_enabled
    bits.push(0); // delta_update
    bits.push(1); // trailing marker bit that must remain unread
    let data = pack_bits(&bits);
    let mut r = BitReader::new(&data);
    let params = parse(&mut r).unwrap();
    assert_eq!(params.level, 0);
    assert_eq!(params.sharpness, 0);
    assert_eq!(r.read_bit().unwrap(), 1);
}

#[test]
fn parse_reads_all_six_deltas_when_update_flags_set() {
    // level=0, sharpness=0, delta_enabled=1, delta_update=1, then 4 ref-delta update flags all 1
    // (each followed by s(6) = 7 bits), then 2 mode-delta update flags all 1 (each s(6)).
    let mut bits = vec![0; 6];
    bits.extend([0; 3]);
    bits.push(1); // delta_enabled
    bits.push(1); // delta_update
    for _ in 0..4 {
        bits.push(1); // update_ref_delta
        bits.extend([0, 0, 0, 0, 0, 1]); // s(6): magnitude 0, sign 1 -> -0 == 0
    }
    for _ in 0..2 {
        bits.push(1); // update_mode_delta
        bits.extend([0, 0, 0, 0, 0, 1]); // s(6)
    }
    bits.push(1); // trailing marker bit that must remain unread
    let data = pack_bits(&bits);
    let mut r = BitReader::new(&data);
    let params = parse(&mut r).unwrap();
    assert_eq!(params.level, 0);
    assert_eq!(params.sharpness, 0);
    assert_eq!(r.read_bit().unwrap(), 1);
}
