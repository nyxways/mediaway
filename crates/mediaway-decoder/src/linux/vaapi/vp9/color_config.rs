//! `frame_sync_code()` (VP9 spec §6.2.3) and `color_config()` (§6.2.2), specialized to this
//! crate's Profile-0-only scope.
//!
//! The general `color_config()` reads `ten_or_twelve_bit` when `Profile >= 2` and
//! `subsampling_x`/`subsampling_y`/`reserved_zero` when `Profile` is `1` or `3` — both
//! unreachable here since `header::parse` already rejects any `Profile != 0` before this module
//! is ever called (Profile 0 always means 8-bit 4:2:0 by spec definition, without reading any
//! extra bits for it). So for this crate's scope, `color_config()` reduces to exactly: read
//! `color_space` (`f(3)`), then (unless `color_space == CS_RGB`, which spec-implies 4:4:4 —
//! incompatible with this crate's NV12-only convention) read `color_range` (`f(1)`).

#![forbid(unsafe_code)]

use crate::DecodeError;
use mediaway_sw::h264::BitReader;

/// VP9 spec's `color_space` enum value for `CS_RGB` — implies 4:4:4 subsampling, rejected by
/// this crate's NV12-only scope.
const CS_RGB: u32 = 7;

/// `frame_sync_code()` (VP9 spec §6.2.3): three fixed sync bytes, `0x49 0x83 0x42`.
pub(super) fn frame_sync_code(r: &mut BitReader<'_>) -> Result<(), DecodeError> {
    let map_err = |_| DecodeError::InvalidInput;
    let b0 = r.read_bits(8).map_err(map_err)?;
    let b1 = r.read_bits(8).map_err(map_err)?;
    let b2 = r.read_bits(8).map_err(map_err)?;
    if b0 != 0x49 || b1 != 0x83 || b2 != 0x42 {
        return Err(DecodeError::InvalidInput);
    }
    Ok(())
}

/// `color_config()` (VP9 spec §6.2.2), Profile-0-only reduction — see module doc.
///
/// # Errors
///
/// Returns [`DecodeError::Unsupported`] for `color_space == CS_RGB` (implies 4:4:4).
pub(super) fn parse(r: &mut BitReader<'_>) -> Result<(), DecodeError> {
    let map_err = |_| DecodeError::InvalidInput;
    let color_space = r.read_bits(3).map_err(map_err)?;
    if color_space == CS_RGB {
        return Err(DecodeError::Unsupported);
    }
    let _color_range = r.read_bit().map_err(map_err)?;
    Ok(())
}

#[cfg(test)]
#[path = "color_config_tests.rs"]
mod tests;
