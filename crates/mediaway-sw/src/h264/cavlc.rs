//! CAVLC residual block decoding (ITU-T H.264 § 9.2), Baseline / I-slice scope.
//!
//! Implements `coeff_token` (Table 9-5), level (`level_prefix`/`level_suffix`, § 9.2.2.1),
//! `total_zeros` (Tables 9-7 to 9-9), and `run_before` (Table 9-10) decoding, then places
//! the resulting coefficients into a raster-order 4x4 (or 2x2 chroma DC) block via the
//! inverse zig-zag scan. Two entry points cover every residual block shape this crate's
//! `I_16x16`/`I_PCM` decode loop needs: [`decode_4x4_residual`] (luma DC, luma AC, chroma AC —
//! all share the generic 4x4 `nC`-context VLC selection) and
//! [`decode_chroma_dc_residual`] (the 2x2 chroma DC block, which always uses the fixed
//! `nC == -1` table and has no zig-zag reordering).
//!
//! Table lookups here are a linear bit-by-bit scan over each VLC table rather than a
//! lookup trie — correctness-first for this first decode slice (see
//! `adr/0001-h264-baseline-decoder-first.md`), not a hot-path-optimized implementation.

#![forbid(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "residual coefficient/index arithmetic below is bounded by CAVLC's own \
              spec-defined ranges (TotalCoeff <= 16, 4x4/2x2 block indices), guarded by \
              explicit range checks or checked_* arithmetic at each cast site"
)]

use super::bitreader::BitReader;
use super::cavlc_tables;
use super::error::H264Error;

/// Longest real codeword in any table this module uses (16 bits, `coeff_token` VLC0/VLC1).
/// Bit-by-bit VLC search gives up past this length — a legitimate "malformed input"
/// rejection since none of our tables define a longer valid codeword.
const MAX_VLC_BITS: u8 = 20;

/// Parse a `"0"`/`"1"` bit-string literal into `(value, bit_length)`, MSB-first. `const fn`
/// so table codewords stay exact copies of the spec's bit patterns (see module docs on
/// [`super::cavlc_tables`]) and the only numeric conversion is done once, at compile time.
const fn parse_bits(s: &str) -> (u32, u8) {
    let bytes = s.as_bytes();
    let mut value: u32 = 0;
    let mut i = 0;
    while i < bytes.len() {
        value = (value << 1) | if bytes[i] == b'1' { 1 } else { 0 };
        i += 1;
    }
    (value, bytes.len() as u8)
}

/// Decode one VLC codeword from `reader` by reading bits one at a time and matching the
/// accumulated bits against `table` entries, optionally restricted to entries whose second
/// field equals `filter_second` (used by `total_zeros`/`run_before`, which are separate
/// codes per fixed `TotalCoeff`/`zerosLeft` context; `coeff_token` passes `None`).
///
/// # Errors
///
/// [`H264Error::UnexpectedEof`] if `reader` runs out of bits; [`H264Error::InvalidCavlcCode`]
/// if no table entry matches within [`MAX_VLC_BITS`] (malformed/adversarial input).
fn decode_vlc(
    reader: &mut BitReader<'_>,
    table: &[(u8, u8, &str)],
    filter_second: Option<u8>,
) -> Result<(u8, u8), H264Error> {
    let mut acc: u32 = 0;
    let mut len: u8 = 0;
    loop {
        if len >= MAX_VLC_BITS {
            return Err(H264Error::InvalidCavlcCode);
        }
        acc = (acc << 1) | reader.read_bit()?;
        len += 1;
        for &(first, second, code_str) in table {
            if let Some(want) = filter_second
                && second != want
            {
                continue;
            }
            let (code, code_len) = parse_bits(code_str);
            if code_len == len && code == acc {
                return Ok((first, second));
            }
        }
    }
}

/// Decode `coeff_token` (Table 9-5) for the given `nC` context: `-1` selects the chroma DC
/// table (fixed context, ITU-T H.264 § 9.2.1), `0..=7` select VLC0/VLC1/VLC2 by range, and
/// `>= 8` uses the fixed-length code. Returns `(TotalCoeff, TrailingOnes)`.
fn decode_coeff_token(reader: &mut BitReader<'_>, nc: i32) -> Result<(u8, u8), H264Error> {
    if nc >= 8 {
        let code = reader.read_bits(6)?;
        return Ok(if code == 3 {
            (0, 0)
        } else {
            (((code >> 2) + 1) as u8, (code & 3) as u8)
        });
    }
    let table: &[(u8, u8, &str)] = match nc {
        -1 => cavlc_tables::COEFF_TOKEN_CHROMA_DC,
        0..=1 => cavlc_tables::COEFF_TOKEN_VLC0,
        2..=3 => cavlc_tables::COEFF_TOKEN_VLC1,
        4..=7 => cavlc_tables::COEFF_TOKEN_VLC2,
        // Never constructed by this crate's nC derivation (§ 9.2.1 only ever yields -1 or
        // a non-negative neighbour-derived count) — a defensive reject, not a spec path.
        _ => return Err(H264Error::InvalidCavlcCode),
    };
    decode_vlc(reader, table, None)
}

/// Read `level_prefix`: a unary code (a run of `0` bits terminated by a `1`); the returned
/// value is the number of `0` bits.
fn read_level_prefix(reader: &mut BitReader<'_>) -> Result<u32, H264Error> {
    let mut count = 0u32;
    while reader.read_bit()? == 0 {
        count = count.checked_add(1).ok_or(H264Error::InvalidCavlcCode)?;
    }
    Ok(count)
}

/// Decode `TotalCoeff` coefficient levels (ITU-T H.264 § 9.2.2, § 9.2.2.1), ordered from
/// highest scan frequency (index `0`) to lowest — the order `coeff_token`'s
/// `trailing_ones` and the level-prefix loop both scan in. Only the first `total_coeff`
/// entries of the returned array are meaningful.
fn decode_levels(
    reader: &mut BitReader<'_>,
    total_coeff: u8,
    trailing_ones: u8,
) -> Result<[i32; 16], H264Error> {
    let mut levels = [0i32; 16];
    for level in levels.iter_mut().take(usize::from(trailing_ones)) {
        *level = if reader.read_bit()? == 0 { 1 } else { -1 };
    }

    let mut suffix_length: u32 = u32::from(total_coeff > 10 && trailing_ones < 3);
    let mut is_first_level = true;
    let remaining = usize::from(total_coeff) - usize::from(trailing_ones);
    for level_slot in levels
        .iter_mut()
        .skip(usize::from(trailing_ones))
        .take(remaining)
    {
        let level_prefix = read_level_prefix(reader)?;
        let level_suffix_size: u32 = if level_prefix == 14 && suffix_length == 0 {
            4
        } else if level_prefix >= 15 {
            level_prefix
                .checked_sub(3)
                .ok_or(H264Error::FieldOverflow)?
        } else {
            suffix_length
        };

        let mut level_code = i64::from(level_prefix.min(15))
            .checked_shl(suffix_length)
            .ok_or(H264Error::FieldOverflow)?;
        if level_suffix_size > 0 {
            let level_suffix = reader.read_bits(level_suffix_size)?;
            level_code = level_code
                .checked_add(i64::from(level_suffix))
                .ok_or(H264Error::FieldOverflow)?;
        }
        if level_prefix >= 15 && suffix_length == 0 {
            level_code = level_code.checked_add(15).ok_or(H264Error::FieldOverflow)?;
        }
        if level_prefix >= 16 {
            let bias = 1i64
                .checked_shl(
                    level_prefix
                        .checked_sub(3)
                        .ok_or(H264Error::FieldOverflow)?,
                )
                .ok_or(H264Error::FieldOverflow)?
                .checked_sub(4096)
                .ok_or(H264Error::FieldOverflow)?;
            level_code = level_code
                .checked_add(bias)
                .ok_or(H264Error::FieldOverflow)?;
        }
        if is_first_level && trailing_ones < 3 {
            level_code = level_code.checked_add(2).ok_or(H264Error::FieldOverflow)?;
        }
        is_first_level = false;

        let level = if level_code % 2 == 0 {
            (level_code + 2) >> 1
        } else {
            (-level_code - 1) >> 1
        };
        let level = i32::try_from(level).map_err(|_err| H264Error::FieldOverflow)?;
        *level_slot = level;

        if suffix_length == 0 {
            suffix_length = 1;
        }
        let threshold = 3u32
            .checked_shl(suffix_length - 1)
            .ok_or(H264Error::FieldOverflow)?;
        if level.unsigned_abs() > threshold && suffix_length < 6 {
            suffix_length += 1;
        }
    }
    Ok(levels)
}

/// Decode `run_before` (Table 9-10) for the given number of remaining zero coefficients.
fn decode_run_before(reader: &mut BitReader<'_>, zeros_left: u8) -> Result<u8, H264Error> {
    let column = zeros_left.min(7);
    Ok(decode_vlc(reader, cavlc_tables::RUN_BEFORE, Some(column))?.0)
}

/// Reconstruct zig-zag scan positions for `total_coeff` decoded `levels` (highest-frequency
/// first) and a decoded `total_zeros`, reading each `run_before` value from `reader` along
/// the way (ITU-T H.264 § 9.2.4). Returns coefficients indexed by scan position (`0` = DC /
/// lowest frequency); only positions `< max_num_coeff` are ever written.
///
/// # Errors
///
/// [`H264Error::InvalidCavlcCode`] if the decoded `run_before` values would place a
/// coefficient at or past `max_num_coeff` (self-inconsistent / adversarial input — a
/// conformant encoder never signals this).
fn reconstruct_scan_positions(
    reader: &mut BitReader<'_>,
    levels: &[i32; 16],
    total_coeff: u8,
    total_zeros: u8,
    max_num_coeff: u8,
) -> Result<[i32; 16], H264Error> {
    let mut scan = [0i32; 16];
    if total_coeff == 0 {
        return Ok(scan);
    }
    let tc = usize::from(total_coeff);
    let mut runs = [0u8; 16];
    let mut zeros_left = total_zeros;
    for run in runs.iter_mut().take(tc - 1) {
        if zeros_left > 0 {
            let decoded = decode_run_before(reader, zeros_left)?;
            *run = decoded;
            zeros_left = zeros_left
                .checked_sub(decoded)
                .ok_or(H264Error::InvalidCavlcCode)?;
        }
    }
    runs[tc - 1] = zeros_left;

    let mut pos: i32 = -1;
    for i in (0..tc).rev() {
        pos = pos
            .checked_add(i32::from(runs[i]) + 1)
            .ok_or(H264Error::InvalidCavlcCode)?;
        if pos < 0 || pos >= i32::from(max_num_coeff) {
            return Err(H264Error::InvalidCavlcCode);
        }
        scan[pos as usize] = levels[i];
    }
    Ok(scan)
}

/// A decoded 4x4-shaped residual block (luma DC, luma AC, or chroma AC), in raster order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Residual4x4 {
    /// Coefficients in raster order (`row * 4 + col`), dequantization not yet applied.
    pub(super) raster: [i32; 16],
    /// `TotalCoeff(coeff_token)` — also the CAVLC neighbour context (`nC`) this block
    /// contributes to blocks decoded after it.
    pub(super) total_coeff: u8,
}

/// Decode one 4x4-shaped residual block (ITU-T H.264 § 7.3.5.3.1 `residual_block_cavlc`,
/// invoked for `Intra16x16DCLevel`, `Intra16x16ACLevel`, `LumaLevel`, and `ChromaACLevel`).
///
/// `nc` is the caller-derived CAVLC neighbour context (§ 9.2.1; never `-1` here — that
/// fixed context is only used by chroma DC, see [`decode_chroma_dc_residual`]). `ac_only`
/// selects `maxNumCoeff == 15` (AC-only blocks, DC coefficient excluded, zig-zag scan
/// starts at index 1) vs `maxNumCoeff == 16` (a full 4x4 block).
///
/// # Errors
///
/// [`H264Error::InvalidCavlcCode`] on a decoded `TotalCoeff` exceeding `maxNumCoeff` or a
/// self-inconsistent zero-run reconstruction; other [`H264Error`] variants propagate from
/// the underlying VLC/bit reads.
pub(super) fn decode_4x4_residual(
    reader: &mut BitReader<'_>,
    nc: i32,
    ac_only: bool,
) -> Result<Residual4x4, H264Error> {
    let max_num_coeff: u8 = if ac_only { 15 } else { 16 };
    let (total_coeff, trailing_ones) = decode_coeff_token(reader, nc)?;
    if total_coeff > max_num_coeff {
        return Err(H264Error::InvalidCavlcCode);
    }
    if total_coeff == 0 {
        return Ok(Residual4x4 {
            raster: [0; 16],
            total_coeff: 0,
        });
    }

    let levels = decode_levels(reader, total_coeff, trailing_ones)?;
    let total_zeros = if total_coeff < max_num_coeff {
        decode_vlc(reader, cavlc_tables::TOTAL_ZEROS_4X4, Some(total_coeff))?.0
    } else {
        0
    };
    let scan =
        reconstruct_scan_positions(reader, &levels, total_coeff, total_zeros, max_num_coeff)?;

    let scan_offset = usize::from(ac_only);
    let mut raster = [0i32; 16];
    for (k, &level) in scan.iter().take(usize::from(max_num_coeff)).enumerate() {
        if level != 0 {
            raster[usize::from(cavlc_tables::ZIGZAG_4X4[k + scan_offset])] = level;
        }
    }
    Ok(Residual4x4 {
        raster,
        total_coeff,
    })
}

/// A decoded chroma DC residual block (the 2x2 Hadamard-transformed DC block, 4:2:0 only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ResidualChromaDc {
    /// Coefficients in raster order `[c(0,0), c(0,1), c(1,0), c(1,1)]`, no zig-zag
    /// reordering (ITU-T H.264 § 8.5.11.1 reads the 2x2 block directly).
    pub(super) c: [i32; 4],
    /// `TotalCoeff(coeff_token)`.
    pub(super) total_coeff: u8,
}

/// Decode the chroma DC residual block (ITU-T H.264 § 7.3.5.3.2 `residual_block_cavlc`
/// with `nC == -1`, `maxNumCoeff == 4`; 4:2:0 / `ChromaArrayType == 1` only).
///
/// # Errors
///
/// Same conditions as [`decode_4x4_residual`].
pub(super) fn decode_chroma_dc_residual(
    reader: &mut BitReader<'_>,
) -> Result<ResidualChromaDc, H264Error> {
    const MAX_NUM_COEFF: u8 = 4;
    let (total_coeff, trailing_ones) =
        decode_vlc(reader, cavlc_tables::COEFF_TOKEN_CHROMA_DC, None)?;
    if total_coeff > MAX_NUM_COEFF {
        return Err(H264Error::InvalidCavlcCode);
    }
    if total_coeff == 0 {
        return Ok(ResidualChromaDc {
            c: [0; 4],
            total_coeff: 0,
        });
    }

    let levels = decode_levels(reader, total_coeff, trailing_ones)?;
    let total_zeros = if total_coeff < MAX_NUM_COEFF {
        decode_vlc(
            reader,
            cavlc_tables::TOTAL_ZEROS_CHROMA_DC,
            Some(total_coeff),
        )?
        .0
    } else {
        0
    };
    let scan =
        reconstruct_scan_positions(reader, &levels, total_coeff, total_zeros, MAX_NUM_COEFF)?;

    let mut c = [0i32; 4];
    c.copy_from_slice(&scan[..4]);
    Ok(ResidualChromaDc { c, total_coeff })
}

#[cfg(test)]
#[path = "cavlc_tests.rs"]
mod tests;
