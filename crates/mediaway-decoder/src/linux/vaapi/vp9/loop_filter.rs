//! `loop_filter_params()` (VP9 spec §6.2.8) — always present for both frame types (not an
//! optional tool the way AV1 skips it for a lossless/intrabc frame; see this crate's own ADR
//! § Scope: "real deltas parsed and passed through — not an optional tool, always active").
//!
//! Real per-reference/per-mode loop-filter delta values (`loop_filter_ref_deltas[4]`/
//! `loop_filter_mode_deltas[2]`) are read here (needed to keep this parser's bit position
//! correct so `header_size_in_bytes` comes out right) but **not stored**: `PictureParameterBufferVP9`
//! (`cros-libva` 0.0.13's real vendored struct, confirmed by reading `buffer/vp9.rs` in full)
//! has no field for them at all — only the picture-level `filter_level`/`sharpness_level`
//! scalars are part of that struct. VA-API's own VP9 decode convention (confirmed via `FFmpeg`'s
//! real `vaapi_vp9.c`) has the driver independently re-parse `uncompressed_header()` from the
//! raw `SliceData` bytes it is given (see `super::super::build_slice_param`'s doc comment), so
//! the deltas' own values never need to leave this parser.

#![forbid(unsafe_code)]

use crate::DecodeError;
use mediaway_sw::h264::BitReader;

use super::bits::s;

/// The two `PictureParameterBufferVP9` fields this crate's loop-filter parse actually feeds —
/// see module doc for why the per-reference/per-mode deltas are read but not retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LoopFilterParams {
    pub(super) level: u8,
    pub(super) sharpness: u8,
}

/// Parse `loop_filter_params()` (VP9 spec §6.2.8).
pub(super) fn parse(r: &mut BitReader<'_>) -> Result<LoopFilterParams, DecodeError> {
    let map_err = |_| DecodeError::InvalidInput;
    let level = u8::try_from(r.read_bits(6).map_err(map_err)?).unwrap_or(0);
    let sharpness = u8::try_from(r.read_bits(3).map_err(map_err)?).unwrap_or(0);
    let delta_enabled = r.read_bit().map_err(map_err)? != 0;
    if delta_enabled {
        let delta_update = r.read_bit().map_err(map_err)? != 0;
        if delta_update {
            for _ in 0..4 {
                let update_ref_delta = r.read_bit().map_err(map_err)? != 0;
                if update_ref_delta {
                    let _loop_filter_ref_delta = s(r, 6)?;
                }
            }
            for _ in 0..2 {
                let update_mode_delta = r.read_bit().map_err(map_err)? != 0;
                if update_mode_delta {
                    let _loop_filter_mode_delta = s(r, 6)?;
                }
            }
        }
    }
    Ok(LoopFilterParams { level, sharpness })
}

#[cfg(test)]
#[path = "loop_filter_tests.rs"]
mod tests;
