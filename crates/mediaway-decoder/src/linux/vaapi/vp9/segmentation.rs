//! `segmentation_params()` (VP9 spec §6.2.11) — parsed only far enough to confirm absence, same
//! "parse far enough to confirm absence" convention as this crate's AV1 sibling
//! (`av1/frame_header.rs`, ADR-0003): `segmentation_enabled` is this crate's sole read; `== 1`
//! is rejected as [`DecodeError::Unsupported`] without parsing the rest of the (much larger)
//! `segmentation_params()` syntax structure, since this ADR's scope never needs it.

#![forbid(unsafe_code)]

use crate::DecodeError;
use mediaway_sw::h264::BitReader;

/// Read `segmentation_enabled` (`f(1)`) and reject if set.
pub(super) fn parse(r: &mut BitReader<'_>) -> Result<(), DecodeError> {
    let enabled = r.read_bit().map_err(|_| DecodeError::InvalidInput)? != 0;
    if enabled {
        return Err(DecodeError::Unsupported);
    }
    Ok(())
}

#[cfg(test)]
#[path = "segmentation_tests.rs"]
mod tests;
