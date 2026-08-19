//! `frame_size()` (VP9 spec §6.2.4), `render_size()` (§6.2.6), and `frame_size_with_refs()`
//! (§6.2.5) — the last is copied verbatim from the real primary spec text this session (see
//! `adr/linux/0004-vaapi-vp9-key-frame-and-inter-decode.md` Addendum, § "closes open question
//! #3's first half").

#![forbid(unsafe_code)]

use crate::DecodeError;
use mediaway_sw::h264::BitReader;

use super::ref_table::RefTable;

/// `frame_size()` (VP9 spec §6.2.4): two `f(16)` fields, each `+1`.
pub(super) fn parse_frame_size(r: &mut BitReader<'_>) -> Result<(u32, u32), DecodeError> {
    let map_err = |_| DecodeError::InvalidInput;
    let width = r.read_bits(16).map_err(map_err)?.saturating_add(1);
    let height = r.read_bits(16).map_err(map_err)?.saturating_add(1);
    Ok((width, height))
}

/// `render_size()` (VP9 spec §6.2.6): defaults to `(width, height)` unless the bitstream signals
/// a different render size.
pub(super) fn parse_render_size(
    r: &mut BitReader<'_>,
    width: u32,
    height: u32,
) -> Result<(u32, u32), DecodeError> {
    let map_err = |_| DecodeError::InvalidInput;
    let different = r.read_bit().map_err(map_err)? != 0;
    if different {
        let render_width = r.read_bits(16).map_err(map_err)?.saturating_add(1);
        let render_height = r.read_bits(16).map_err(map_err)?.saturating_add(1);
        Ok((render_width, render_height))
    } else {
        Ok((width, height))
    }
}

/// `frame_size_with_refs()` (VP9 spec §6.2.5), real, copied verbatim from the primary spec text
/// (see module doc): tries each of the three references in turn (`found_ref f(1)` per
/// reference, stopping at the first hit); falls back to `frame_size()` if none match. This
/// crate's own single-forward-reference-shaped scope means only `ref_frame_idx[0]` (`LAST`) is
/// ever meaningfully populated in practice, but every reference is still read (or not read, per
/// the spec's own `break`-on-first-hit shape) exactly as the real syntax table requires — see
/// `adr/linux/0004-vaapi-vp9-key-frame-and-inter-decode.md` Addendum.
pub(super) fn parse_frame_size_with_refs(
    r: &mut BitReader<'_>,
    ref_frame_idx: [u8; 3],
    ref_table: &RefTable,
) -> Result<(u32, u32), DecodeError> {
    let map_err = |_| DecodeError::InvalidInput;
    let mut found: Option<(u32, u32)> = None;
    for &idx in &ref_frame_idx {
        let found_ref = r.read_bit().map_err(map_err)? != 0;
        if found_ref {
            let size = ref_table
                .size(usize::from(idx))
                .ok_or(DecodeError::InvalidInput)?;
            found = Some(size);
            break;
        }
    }
    let (width, height) = match found {
        Some(size) => size,
        None => parse_frame_size(r)?,
    };
    parse_render_size(r, width, height)
}

#[cfg(test)]
#[path = "frame_size_tests.rs"]
mod tests;
