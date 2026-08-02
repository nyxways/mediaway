//! ISO/IEC 23001-7 sample crypto — `cenc` (AES-128-CTR) first.
//!
//! Counter rules (CTR): each encrypted 16-byte block consumes one counter
//! increment. Bytes in clear subsample ranges do **not** advance the counter.
//! Partial final blocks of a protected range still consume one keystream block
//! (unused keystream bytes discarded).

#![forbid(unsafe_code)]

use crate::Error;
use aes::Aes128;
use aes::cipher::{BlockCipherEncrypt, KeyInit};

type Block16 = aes::cipher::Block<Aes128>;

/// Protection scheme (`schm.scheme_type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Scheme {
    /// AES-128 CTR, full protected ranges (`cenc`).
    Cenc,
}

/// Crypt/skip block pattern (`tenc` / pattern encryption).
///
/// [`Pattern::NONE`] means full-region encryption within each protected range
/// (`cenc` / `cbc1`). Non-zero patterns are for `cens` / `cbcs` (not Stage 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pattern {
    /// Encrypt this many 16-byte blocks, then skip.
    pub crypt_blocks: u8,
    /// Leave this many 16-byte blocks clear (pattern schemes).
    pub skip_blocks: u8,
}

impl Pattern {
    /// No pattern — encrypt every block in each protected range.
    pub const NONE: Self = Self {
        crypt_blocks: 0,
        skip_blocks: 0,
    };

    /// True when this is full-region (no crypt/skip pattern).
    #[must_use]
    pub const fn is_none(self) -> bool {
        self.crypt_blocks == 0
    }
}

/// One subsample: clear bytes then protected bytes (ISO CENC subsample).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Subsample {
    /// Leading clear (unencrypted) byte count.
    pub clear_bytes: u16,
    /// Following protected byte count.
    pub protected_bytes: u32,
}

/// Decrypt a sample in place under [`Scheme::Cenc`].
///
/// `iv` is the 16-byte CTR initialization block. For an 8-byte per-sample IV,
/// place the IV in the high 8 bytes and zero the low 8 (ISO CTR construction).
///
/// Empty `subsamples` means the entire `data` buffer is one protected range.
pub fn decrypt_cenc(
    key: &[u8; 16],
    iv: &[u8; 16],
    pattern: Pattern,
    data: &mut [u8],
    subsamples: &[Subsample],
) -> Result<(), Error> {
    apply_cenc(key, iv, pattern, data, subsamples)
}

/// Encrypt a sample in place under [`Scheme::Cenc`] (same CTR keystream as decrypt).
pub fn encrypt_cenc(
    key: &[u8; 16],
    iv: &[u8; 16],
    pattern: Pattern,
    data: &mut [u8],
    subsamples: &[Subsample],
) -> Result<(), Error> {
    apply_cenc(key, iv, pattern, data, subsamples)
}

fn apply_cenc(
    key: &[u8; 16],
    iv: &[u8; 16],
    pattern: Pattern,
    data: &mut [u8],
    subsamples: &[Subsample],
) -> Result<(), Error> {
    if !pattern.is_none() {
        // Stage 1: `cenc` only — pattern schemes come later.
        return Err(Error::InvalidPattern);
    }
    let cipher = Aes128::new(&(*key).into());
    let mut counter = *iv;
    if subsamples.is_empty() {
        xor_ctr(&cipher, &mut counter, data);
        return Ok(());
    }
    let mut pos = 0usize;
    for sub in subsamples {
        let clear = usize::from(sub.clear_bytes);
        let protected = sub.protected_bytes as usize;
        let end_clear = pos.checked_add(clear).ok_or(Error::SubsampleOverflow)?;
        let end_prot = end_clear
            .checked_add(protected)
            .ok_or(Error::SubsampleOverflow)?;
        if end_prot > data.len() {
            return Err(Error::SubsampleOverflow);
        }
        // Clear range: leave bytes alone; do not advance CTR.
        pos = end_clear;
        if protected > 0 {
            xor_ctr(&cipher, &mut counter, &mut data[pos..end_prot]);
        }
        pos = end_prot;
    }
    Ok(())
}

fn xor_ctr(cipher: &Aes128, counter: &mut [u8; 16], data: &mut [u8]) {
    let mut offset = 0;
    while offset < data.len() {
        let mut block: Block16 = (*counter).into();
        cipher.encrypt_block(&mut block);
        let n = (data.len() - offset).min(16);
        for i in 0..n {
            data[offset + i] ^= block[i];
        }
        offset += n;
        inc_be128(counter);
    }
}

/// Big-endian 128-bit counter increment (wraps).
fn inc_be128(block: &mut [u8; 16]) {
    for i in (0..16).rev() {
        let (v, overflow) = block[i].overflowing_add(1);
        block[i] = v;
        if !overflow {
            break;
        }
    }
}

/// Build a 16-byte CTR IV from an 8-byte per-sample IV (high 8 = IV, low 8 = 0).
#[must_use]
pub fn iv_from_8(iv8: &[u8; 8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(iv8);
    out
}

/// Build a 16-byte CTR IV from a constant IV of size 8 or 16.
pub fn iv_from_constant(constant_iv: &[u8]) -> Result<[u8; 16], Error> {
    match constant_iv.len() {
        8 => {
            let mut a = [0u8; 8];
            a.copy_from_slice(constant_iv);
            Ok(iv_from_8(&a))
        }
        16 => {
            let mut a = [0u8; 16];
            a.copy_from_slice(constant_iv);
            Ok(a)
        }
        _ => Err(Error::InvalidKeyMaterial),
    }
}

#[cfg(test)]
#[path = "cenc_tests.rs"]
mod tests;
