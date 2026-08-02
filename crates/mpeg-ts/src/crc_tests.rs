//! Unit tests for the MPEG-2 PSI CRC-32 variant.

#![cfg(test)]

use super::crc32_mpeg2;

#[test]
fn is_deterministic() {
    assert_eq!(crc32_mpeg2(b"pat section"), crc32_mpeg2(b"pat section"));
}

#[test]
fn single_bit_change_changes_the_crc() {
    assert_ne!(crc32_mpeg2(b"pat section"), crc32_mpeg2(b"pat sectipn"));
}

#[test]
fn differs_from_ogg_variant_on_same_input() {
    // Same polynomial as `ogg`'s CRC, but a different init value (0xFFFFFFFF vs
    // 0) — the two variants must not collide on typical input.
    let ogg_style_init0 = {
        let mut crc: u32 = 0;
        for &byte in b"crc variant check" {
            crc ^= u32::from(byte) << 24;
            for _ in 0..8 {
                crc = if crc & 0x8000_0000 == 0 {
                    crc << 1
                } else {
                    (crc << 1) ^ 0x04C1_1DB7
                };
            }
        }
        crc
    };
    assert_ne!(crc32_mpeg2(b"crc variant check"), ogg_style_init0);
}
