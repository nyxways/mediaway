//! Ogg's CRC-32 variant (RFC 3533 §6): polynomial `0x04C11DB7`, MSB-first, no
//! input/output reflection, init `0`, no final XOR — **not** the same variant as
//! zlib/PNG `crc32` (which is bit-reflected).

#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "crate-private helper used by mux.rs/demux.rs; module itself is private"
)]

const POLY: u32 = 0x04C1_1DB7;

/// Compute the Ogg page CRC over `data` (caller must zero the page's own CRC
/// field before calling, per RFC 3533).
#[must_use]
pub(crate) fn crc32_ogg(data: &[u8]) -> u32 {
    let mut crc: u32 = 0;
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
