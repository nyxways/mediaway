//! `uncompressed_header()` (VP9 spec §6.2), copied verbatim from the real primary spec text this
//! session (see `adr/linux/0004-vaapi-vp9-key-frame-and-inter-decode.md` Addendum) and
//! specialized to this crate's own scope: `Profile == 0` only, `show_existing_frame == 0`,
//! `show_frame == 1` required (so `intra_only` is always spec-inferred `0` and its own branch is
//! never reached — see the ADR's own § Scope note on this), no segmentation, no lossless, single
//! tile.
//!
//! Ties together every sibling module in this directory, in the real spec's own field order.

#![forbid(unsafe_code)]

use crate::DecodeError;
use mediaway_sw::h264::BitReader;

use super::color_config;
use super::frame_size::{self, parse_frame_size, parse_render_size};
use super::loop_filter::{self, LoopFilterParams};
use super::quantization::{self, QuantizationParams};
use super::ref_table::RefTable;
use super::segmentation;
use super::tile_info;

/// `read_interpolation_filter()`'s `SWITCHABLE` sentinel (VP9 spec §6.2.7) — `interpolation_filter`
/// values `0..=3` (`EIGHTTAP_SMOOTH`/`EIGHTTAP`/`EIGHTTAP_SHARP`/`BILINEAR`) come directly from
/// the 2-bit `raw_interpolation_filter` field: VP9's own `literal_to_filter[]` permutation table
/// happens to be the identity mapping for those four values (general VP9 domain knowledge, not
/// itself primary-source-quoted this session).
const INTERPOLATION_FILTER_SWITCHABLE: u8 = 4;

fn read_interpolation_filter(r: &mut BitReader<'_>) -> Result<u8, DecodeError> {
    let map_err = |_| DecodeError::InvalidInput;
    let is_filter_switchable = r.read_bit().map_err(map_err)? != 0;
    if is_filter_switchable {
        return Ok(INTERPOLATION_FILTER_SWITCHABLE);
    }
    let raw = r.read_bits(2).map_err(map_err)?;
    u8::try_from(raw).map_err(|_| DecodeError::InvalidInput)
}

/// Parsed `uncompressed_header()` fields this crate's VA-API decode parameter buffers need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each bool names one VP9 uncompressed_header() syntax element or spec-derived \
              flag — a 1:1 spec mapping this crate relies on for review, same precedent as this \
              crate's H.264 Pps / AV1 FrameHeader"
)]
pub(super) struct Header {
    pub(super) is_key: bool,
    pub(super) error_resilient_mode: bool,
    pub(super) refresh_frame_flags: u8,
    pub(super) ref_frame_idx: [u8; 3],
    pub(super) ref_frame_sign_bias: [bool; 3],
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) allow_high_precision_mv: bool,
    pub(super) interpolation_filter: u8,
    pub(super) refresh_frame_context: bool,
    pub(super) frame_parallel_decoding_mode: bool,
    pub(super) frame_context_idx: u8,
    pub(super) reset_frame_context: u8,
    pub(super) loop_filter: LoopFilterParams,
    pub(super) quantization: QuantizationParams,
    /// `header_size_in_bytes` (`f(16)`) — the compressed header's own byte length, usable
    /// directly as `first_partition_size` (no extra computation, per this crate's own ADR).
    pub(super) first_partition_size: u16,
    /// This header's own byte length, rounded up to the next byte boundary
    /// (`frame_header_length_in_bytes`) — `bits_consumed.div_ceil(8)`, mirroring this crate's
    /// AV1 sibling's `byte_alignment()` convention.
    pub(super) frame_header_length_in_bytes: usize,
}

impl Header {
    /// Parse one VP9 frame's `uncompressed_header()` from `data` (the whole packet payload —
    /// this crate assumes one [`Packet`](mediaway_common::Packet) carries exactly one VP9
    /// frame's bitstream, matching this workspace's own VP9 encoder sibling's output and every
    /// other codec's framing convention in this crate; see the ADR's own § Scope). `ref_table`
    /// is this session's persistent 8-slot shadow table, needed by `frame_size_with_refs()` for
    /// an `INTER_FRAME`.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::InvalidInput`] on truncated/malformed data, or
    /// [`DecodeError::Unsupported`] for anything outside this crate's scope (see the module doc).
    #[allow(
        clippy::too_many_lines,
        reason = "one linear, spec-section-ordered read sequence (uncompressed_header() plus \
                  its called sub-syntax-structures); splitting the sub-structures into helper \
                  modules (color_config/frame_size/loop_filter/quantization/segmentation/ \
                  tile_info, each already its own file) already keeps each individually short — \
                  the remainder is uncompressed_header()'s own single top-level control flow, \
                  which has no independently reusable pieces, mirroring this crate's AV1 sibling's \
                  identical allow/reasoning for FrameHeader::parse"
    )]
    pub(super) fn parse(data: &[u8], ref_table: &RefTable) -> Result<Self, DecodeError> {
        let mut r = BitReader::new(data);
        let map_err = |_| DecodeError::InvalidInput;

        let _frame_marker = r.read_bits(2).map_err(map_err)?;
        let profile_low_bit = r.read_bit().map_err(map_err)?;
        let profile_high_bit = r.read_bit().map_err(map_err)?;
        let profile = (profile_high_bit << 1) + profile_low_bit;
        if profile != 0 {
            // Profile 0 only (8-bit 4:2:0) — see this crate's own ADR § Scope. Profile != 0
            // means `reserved_zero` (read only when Profile == 3) is never reachable from here.
            return Err(DecodeError::Unsupported);
        }

        let show_existing_frame = r.read_bit().map_err(map_err)? != 0;
        if show_existing_frame {
            return Err(DecodeError::Unsupported);
        }
        let is_key = r.read_bit().map_err(map_err)? == 0;
        let show_frame = r.read_bit().map_err(map_err)? != 0;
        if !show_frame {
            return Err(DecodeError::Unsupported);
        }
        let error_resilient_mode = r.read_bit().map_err(map_err)? != 0;

        let refresh_frame_flags;
        let mut ref_frame_idx = [0u8; 3];
        let mut ref_frame_sign_bias = [false; 3];
        let width;
        let height;
        let mut allow_high_precision_mv = false;
        let mut interpolation_filter = 0u8;
        let reset_frame_context;

        if is_key {
            color_config::frame_sync_code(&mut r)?;
            color_config::parse(&mut r)?;
            let (w, h) = parse_frame_size(&mut r)?;
            let (_rw, _rh) = parse_render_size(&mut r, w, h)?;
            width = w;
            height = h;
            refresh_frame_flags = 0xffu8;
            reset_frame_context = 0u8;
        } else {
            // show_frame == 1 is required (rejected above), so intra_only is always
            // spec-inferred 0 and never read — this crate's scope never reaches the
            // intra_only == 1 branch (see module doc).
            reset_frame_context = if error_resilient_mode {
                0u8
            } else {
                u8::try_from(r.read_bits(2).map_err(map_err)?).unwrap_or(0)
            };
            let flags = r.read_bits(8).map_err(map_err)?;
            refresh_frame_flags = u8::try_from(flags).unwrap_or(0);
            for i in 0..3 {
                ref_frame_idx[i] = u8::try_from(r.read_bits(3).map_err(map_err)?).unwrap_or(0);
                ref_frame_sign_bias[i] = r.read_bit().map_err(map_err)? != 0;
            }
            let (w, h) = frame_size::parse_frame_size_with_refs(&mut r, ref_frame_idx, ref_table)?;
            width = w;
            height = h;
            allow_high_precision_mv = r.read_bit().map_err(map_err)? != 0;
            interpolation_filter = read_interpolation_filter(&mut r)?;
        }

        let (refresh_frame_context, frame_parallel_decoding_mode) = if error_resilient_mode {
            (false, true)
        } else {
            (
                r.read_bit().map_err(map_err)? != 0,
                r.read_bit().map_err(map_err)? != 0,
            )
        };
        let frame_context_idx = u8::try_from(r.read_bits(2).map_err(map_err)?).unwrap_or(0);

        let loop_filter = loop_filter::parse(&mut r)?;
        let quantization = quantization::parse(&mut r)?;
        if quantization.lossless {
            return Err(DecodeError::Unsupported);
        }
        segmentation::parse(&mut r)?;

        // VP9's mode-info unit is 8x8 (unlike AV1's 4x4) — VP9 spec §7.2.6 compute_image_size().
        let mi_cols = width.div_ceil(8);
        tile_info::parse(&mut r, mi_cols)?;

        let header_size_in_bytes = r.read_bits(16).map_err(map_err)?;
        if header_size_in_bytes == 0 {
            // VP9 spec: "It is a requirement of bitstream conformance that header_size_in_bytes
            // is greater than 0."
            return Err(DecodeError::InvalidInput);
        }
        let first_partition_size =
            u16::try_from(header_size_in_bytes).map_err(|_| DecodeError::InvalidInput)?;

        let frame_header_length_in_bytes = r.bits_read().div_ceil(8);

        Ok(Self {
            is_key,
            error_resilient_mode,
            refresh_frame_flags,
            ref_frame_idx,
            ref_frame_sign_bias,
            width,
            height,
            allow_high_precision_mv,
            interpolation_filter,
            refresh_frame_context,
            frame_parallel_decoding_mode,
            frame_context_idx,
            reset_frame_context,
            loop_filter,
            quantization,
            first_partition_size,
            frame_header_length_in_bytes,
        })
    }
}

#[cfg(test)]
#[path = "header_tests.rs"]
mod tests;
