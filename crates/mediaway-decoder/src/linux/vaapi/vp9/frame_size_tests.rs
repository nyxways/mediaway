#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap / expect"
)]

use super::*;

#[test]
fn parse_frame_size_reads_width_minus_1_height_minus_1() {
    // width_minus_1 = 63 (0x003F), height_minus_1 = 47 (0x002F).
    let data = [0x00, 0x3F, 0x00, 0x2F];
    let mut r = BitReader::new(&data);
    let (w, h) = parse_frame_size(&mut r).unwrap();
    assert_eq!(w, 64);
    assert_eq!(h, 48);
}

#[test]
fn parse_render_size_defaults_when_flag_clear() {
    let data = [0b0000_0000];
    let mut r = BitReader::new(&data);
    let (rw, rh) = parse_render_size(&mut r, 64, 48).unwrap();
    assert_eq!((rw, rh), (64, 48));
}

#[test]
fn parse_render_size_reads_explicit_size_when_flag_set() {
    // different=1, then render_width_minus_1=31 (0x001F), render_height_minus_1=23 (0x0017),
    // bit-packed starting with the leading 1 flag bit.
    let mut bits = vec![1u8];
    for b in 0x001Fu16.to_be_bytes() {
        for i in (0..8).rev() {
            bits.push((b >> i) & 1);
        }
    }
    for b in 0x0017u16.to_be_bytes() {
        for i in (0..8).rev() {
            bits.push((b >> i) & 1);
        }
    }
    let data = pack_bits(&bits);
    let mut r = BitReader::new(&data);
    let (rw, rh) = parse_render_size(&mut r, 64, 48).unwrap();
    assert_eq!((rw, rh), (32, 24));
}

#[test]
fn frame_size_with_refs_uses_first_found_ref() {
    let mut table = RefTable::new();
    table.refresh(0b0000_0001, 0, 320, 240); // slot 0 -> pool 0, 320x240
    // found_ref[0]=1 -> stop, use ref_frame_idx[0]=0's size; then render_size() reads the
    // "different" flag = 0.
    let data = [0b1000_0000];
    let mut r = BitReader::new(&data);
    let (w, h) = parse_frame_size_with_refs(&mut r, [0, 0, 0], &table).unwrap();
    assert_eq!((w, h), (320, 240));
}

#[test]
fn frame_size_with_refs_falls_back_to_frame_size_when_no_ref_found() {
    let table = RefTable::new();
    // found_ref[0]=0, found_ref[1]=0, found_ref[2]=0, then frame_size(): width_minus_1=15,
    // height_minus_1=11, then render_size() "different" flag = 0.
    let mut bits = vec![0u8, 0u8, 0u8]; // three found_ref bits, all 0
    for b in 15u16.to_be_bytes() {
        for i in (0..8).rev() {
            bits.push((b >> i) & 1);
        }
    }
    for b in 11u16.to_be_bytes() {
        for i in (0..8).rev() {
            bits.push((b >> i) & 1);
        }
    }
    bits.push(0); // render_size "different" flag
    let data = pack_bits(&bits);
    let mut r = BitReader::new(&data);
    let (w, h) = parse_frame_size_with_refs(&mut r, [0, 1, 2], &table).unwrap();
    assert_eq!((w, h), (16, 12));
}

#[test]
fn frame_size_with_refs_errors_when_ref_slot_unpopulated() {
    let table = RefTable::new(); // empty — no slot ever refreshed
    let data = [0b1_0000000]; // found_ref[0] = 1, but table has nothing at slot 0
    let mut r = BitReader::new(&data);
    let result = parse_frame_size_with_refs(&mut r, [0, 0, 0], &table);
    assert_eq!(result, Err(DecodeError::InvalidInput));
}

/// Pack a `Vec<u8>` of individual bit values (0/1, MSB-first) into bytes, zero-padding the last
/// byte if needed — small test helper for hand-constructing bit-exact fixtures.
fn pack_bits(bits: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; bits.len().div_ceil(8)];
    for (i, &bit) in bits.iter().enumerate() {
        if bit != 0 {
            out[i / 8] |= 1 << (7 - (i % 8));
        }
    }
    out
}
