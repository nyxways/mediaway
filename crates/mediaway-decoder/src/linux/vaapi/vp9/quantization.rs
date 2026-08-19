//! `quantization_params()`/`read_delta_q()` (VP9 spec §6.2.9-6.2.10), copied verbatim from the
//! real primary spec text this session (see
//! `adr/linux/0004-vaapi-vp9-key-frame-and-inter-decode.md` Addendum). Field names
//! (`delta_q_y_dc`/`delta_q_uv_dc`/`delta_q_uv_ac`) match the spec's own three-delta shape —
//! narrower than AV1's five-delta (`delta_q_{y,u,v}_{dc,ac}`) `quantization_params()`.

#![forbid(unsafe_code)]

use crate::DecodeError;
use mediaway_sw::h264::BitReader;

use super::bits::s;

/// `quantization_params()` fields this crate needs; `lossless` is the derived
/// `Lossless = (all four deltas/base_q_idx == 0)` value (VP9 spec §6.2.9) — this crate's own
/// scope rejects a lossless frame outright (see `header::parse`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct QuantizationParams {
    pub(super) base_q_idx: u8,
    pub(super) lossless: bool,
}

/// `read_delta_q()` (VP9 spec §6.2.10): a coded flag, then `s(4)` when set.
fn read_delta_q(r: &mut BitReader<'_>) -> Result<i8, DecodeError> {
    let map_err = |_| DecodeError::InvalidInput;
    let delta_coded = r.read_bit().map_err(map_err)? != 0;
    if !delta_coded {
        return Ok(0);
    }
    let value = s(r, 4)?;
    i8::try_from(value).map_err(|_| DecodeError::InvalidInput)
}

/// Parse `quantization_params()` (VP9 spec §6.2.9).
#[allow(
    clippy::similar_names,
    reason = "delta_q_uv_dc/delta_q_uv_ac are the VP9 spec's own quantization_params() names \
              (§6.2.9) — a 1:1 spec mapping this crate relies on for review, same precedent as \
              this crate's AV1 sibling's parse_quantization_params allow"
)]
pub(super) fn parse(r: &mut BitReader<'_>) -> Result<QuantizationParams, DecodeError> {
    let map_err = |_| DecodeError::InvalidInput;
    let base_q_idx = u8::try_from(r.read_bits(8).map_err(map_err)?).unwrap_or(0);
    let delta_q_y_dc = read_delta_q(r)?;
    let delta_q_uv_dc = read_delta_q(r)?;
    let delta_q_uv_ac = read_delta_q(r)?;
    let lossless = base_q_idx == 0 && delta_q_y_dc == 0 && delta_q_uv_dc == 0 && delta_q_uv_ac == 0;
    Ok(QuantizationParams {
        base_q_idx,
        lossless,
    })
}

#[cfg(test)]
#[path = "quantization_tests.rs"]
mod tests;
