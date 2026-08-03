//! Unit tests for the Ogg CRC-32 variant.

#![cfg(test)]

use super::crc32_ogg;

#[test]
fn empty_input_is_zero() {
    assert_eq!(crc32_ogg(&[]), 0);
}

#[test]
fn differs_from_zero_for_nonempty_input() {
    assert_ne!(crc32_ogg(b"OggS"), 0);
}

#[test]
fn is_deterministic() {
    assert_eq!(crc32_ogg(b"hello ogg"), crc32_ogg(b"hello ogg"));
}

#[test]
fn single_bit_change_changes_the_crc() {
    assert_ne!(crc32_ogg(b"hello ogg"), crc32_ogg(b"hellp ogg"));
}
