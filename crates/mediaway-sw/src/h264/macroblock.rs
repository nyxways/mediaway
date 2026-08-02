//! H.264 macroblock-layer parsing for I-slices (ITU-T H.264 § 7.3.5, Table 7-11).
//!
//! Scope cut: `mb_type == 0` (`I_NxN`, 4x4/8x8 intra prediction) is recognized but its
//! pixel reconstruction is not implemented — [`super::decode`] rejects it with
//! [`super::H264Error::UnsupportedMbType`] rather than guessing at 4x4-block-level
//! reconstruction. `I_16x16` (`mb_type` `1..=24`) and `I_PCM` (`mb_type == 25`) are fully
//! parsed and reconstructed. See `adr/0003-cavlc-i-slice-first-decode.md`.

#![forbid(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    reason = "mb_type arithmetic below is bounded by the ue(v) match arms (0..=25) that \
              guard every cast site; values provably fit the narrower integer type"
)]

use super::bitreader::BitReader;
use super::error::H264Error;

/// Decoded `mb_type` for an I-slice macroblock (ITU-T H.264 Table 7-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MbType {
    /// `mb_type == 0`: `I_NxN` (4x4 or 8x8 intra prediction). Recognized but not
    /// reconstructed by this crate — see the module docs.
    INxN,
    /// `mb_type` `1..=24`: `I_16x16`, with prediction mode and coded-block-pattern fields
    /// derived directly from `mb_type` (no separate `coded_block_pattern` syntax element).
    I16x16 {
        /// `Intra16x16PredMode` (`0` = Vertical, `1` = Horizontal, `2` = DC, `3` = Plane).
        pred_mode: u8,
        /// `CodedBlockPatternLuma`: `0` (no AC residual) or `15` (all 16 4x4 luma blocks
        /// may carry AC residual).
        cbp_luma: u8,
        /// `CodedBlockPatternChroma`: `0` (no chroma residual), `1` (chroma DC only), or
        /// `2` (chroma DC + AC).
        cbp_chroma: u8,
    },
    /// `mb_type == 25`: `I_PCM` — raw, unquantized sample values follow byte-aligned.
    IPcm,
}

impl MbType {
    /// Decode a raw I-slice `mb_type` value (already read as `ue(v)`) into its semantics.
    ///
    /// # Errors
    ///
    /// Returns [`H264Error::InvalidMbType`] for `mb_type > 25`.
    pub const fn from_raw(mb_type: u32) -> Result<Self, H264Error> {
        match mb_type {
            0 => Ok(Self::INxN),
            1..=24 => {
                let base = mb_type - 1;
                let pred_mode = (base % 4) as u8;
                let cbp_chroma = ((base / 4) % 3) as u8;
                let cbp_luma = if base / 12 == 0 { 0 } else { 15 };
                Ok(Self::I16x16 {
                    pred_mode,
                    cbp_luma,
                    cbp_chroma,
                })
            }
            25 => Ok(Self::IPcm),
            _ => Err(H264Error::InvalidMbType),
        }
    }
}

/// `intra_chroma_pred_mode` (ITU-T H.264 § 7.3.5.1): `ue(v)` in `0..=3`.
///
/// # Errors
///
/// Propagates [`BitReader::read_ue`] errors; the H.264 spec places no upper bound on the
/// codeword itself, so an out-of-range decoded value (`> 3`) is also rejected as
/// [`H264Error::InvalidMbType`] (malformed input, not a distinct error variant since this
/// is part of the same macroblock-header parse).
pub(super) fn read_intra_chroma_pred_mode(reader: &mut BitReader<'_>) -> Result<u8, H264Error> {
    let mode = reader.read_ue()?;
    u8::try_from(mode)
        .ok()
        .filter(|&m| m <= 3)
        .ok_or(H264Error::InvalidMbType)
}

#[cfg(test)]
#[path = "macroblock_tests.rs"]
mod tests;
