//! MPEG-2 PSI section CRC-32 variant (ISO/IEC 13818-1 Annex A / ITU-T H.222.0):
//! polynomial `0x04C11DB7`, MSB-first, no input/output reflection, init
//! `0xFFFFFFFF`, no final XOR. Same polynomial as Ogg's CRC (`ogg` crate) but a
//! **different init value** — the two are not interchangeable.

#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "crate-private helper used by psi.rs; module itself is private"
)]

const POLY: u32 = 0x04C1_1DB7;

/// Compute the MPEG-2 PSI section CRC-32 over `data` (the section bytes up to
/// but not including the trailing 4-byte CRC field itself).
#[must_use]
pub(crate) fn crc32_mpeg2(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= u32::from(byte) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ POLY
            };
        }
    }
    crc
}

#[cfg(test)]
#[path = "crc_tests.rs"]
mod tests;
