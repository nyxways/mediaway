//! Bit-level reader for H.264 RBSP parsing (fixed-width fields and Exp-Golomb codes).

#![forbid(unsafe_code)]

use super::error::H264Error;

/// Reads bits MSB-first from an RBSP byte slice (emulation-prevention bytes already
/// removed by the caller, e.g. via [`super::NalUnit::parse`]).
#[derive(Debug)]
pub struct BitReader<'a> {
    data: &'a [u8],
    bit_pos: usize,
}

impl<'a> BitReader<'a> {
    /// Wrap `data` (already de-emulated RBSP bytes) for bit-level reading, starting at
    /// the first bit of the first byte.
    #[must_use]
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data, bit_pos: 0 }
    }

    /// Number of bits consumed so far.
    #[must_use]
    pub const fn bits_read(&self) -> usize {
        self.bit_pos
    }

    /// Read a single bit (0 or 1).
    ///
    /// # Errors
    ///
    /// Returns [`H264Error::UnexpectedEof`] once past the end of the buffer.
    pub fn read_bit(&mut self) -> Result<u32, H264Error> {
        let byte_index = self.bit_pos / 8;
        let byte = *self.data.get(byte_index).ok_or(H264Error::UnexpectedEof)?;
        let shift = 7 - (self.bit_pos % 8);
        self.bit_pos += 1;
        Ok(u32::from((byte >> shift) & 1))
    }

    /// Read `count` bits (`0..=32`) as an unsigned integer, MSB first.
    ///
    /// # Errors
    ///
    /// Returns [`H264Error::UnexpectedEof`] if fewer than `count` bits remain.
    pub fn read_bits(&mut self, count: u32) -> Result<u32, H264Error> {
        let mut value = 0u32;
        for _ in 0..count {
            value = (value << 1) | self.read_bit()?;
        }
        Ok(value)
    }

    /// Read an unsigned Exp-Golomb code (`ue(v)`, ITU-T H.264 § 9.1).
    ///
    /// # Errors
    ///
    /// Returns [`H264Error::UnexpectedEof`] on truncated input, or
    /// [`H264Error::ExpGolombOverflow`] when the code's leading-zero prefix or decoded
    /// value would not fit in `u32`.
    pub fn read_ue(&mut self) -> Result<u32, H264Error> {
        let mut leading_zero_bits = 0u32;
        while self.read_bit()? == 0 {
            leading_zero_bits += 1;
            if leading_zero_bits >= u32::BITS {
                return Err(H264Error::ExpGolombOverflow);
            }
        }
        if leading_zero_bits == 0 {
            return Ok(0);
        }
        let suffix = self.read_bits(leading_zero_bits)?;
        let value = 1u32
            .checked_shl(leading_zero_bits)
            .and_then(|v| v.checked_sub(1))
            .and_then(|v| v.checked_add(suffix))
            .ok_or(H264Error::ExpGolombOverflow)?;
        Ok(value)
    }

    /// Read a signed Exp-Golomb code (`se(v)`, ITU-T H.264 § 9.1.1).
    ///
    /// # Errors
    ///
    /// Propagates [`read_ue`](Self::read_ue) errors, plus
    /// [`H264Error::ExpGolombOverflow`] if the magnitude does not fit in `i32`.
    pub fn read_se(&mut self) -> Result<i32, H264Error> {
        let code = self.read_ue()?;
        let magnitude = code.div_ceil(2);
        let magnitude = i32::try_from(magnitude).map_err(|_err| H264Error::ExpGolombOverflow)?;
        Ok(if code % 2 == 0 { -magnitude } else { magnitude })
    }
}

#[cfg(test)]
#[path = "bitreader_tests.rs"]
mod tests;
