//! `sequence_header_obu()` parsing (AV1 spec §5.5.1 `sequence_header_obu()`, §5.5.2
//! `color_config()`), restricted to this crate's decode scope: Main profile (`seq_profile ==
//! 0`), 8-bit (`!high_bitdepth`), 4:2:0/non-monochrome (implied by `seq_profile == 0` once
//! `mono_chrome` is rejected), `reduced_still_picture_header == 0`, single operating point, no
//! `timing_info`/`decoder_model_info`, no `frame_id_numbers`, and every optional coding tool
//! (`enable_filter_intra`/`enable_intra_edge_filter`/`enable_interintra_compound`/
//! `enable_masked_compound`/`enable_warped_motion`/`enable_dual_filter`/`enable_jnt_comp`/
//! `enable_ref_frame_mvs`/`enable_superres`/`enable_cdef`/`enable_restoration`/
//! `film_grain_params_present`) rejected if signaled. See
//! [ADR-0005](../../../../adr/linux/0005-vaapi-av1-key-frame-decode.md) § Scope.
//!
//! Field presence/order cross-checked (not copied — a reader and a writer are structurally
//! different code) against `windows::d3d12_video_encode::bitstream_av1::write_sequence_header`
//! (`bitstream_av1.rs:132-192`), which already enumerates every conditional-inference rule for
//! this exact profile in its own inline comments.

#![forbid(unsafe_code)]

use crate::DecodeError;
use mediaway_sw::h264::BitReader;

/// `seq_force_screen_content_tools` / `seq_force_integer_mv` sentinel meaning "read per-frame"
/// (AV1 spec §6.4.1, `SELECT_SCREEN_CONTENT_TOOLS` / `SELECT_INTEGER_MV`, both value `2`).
const SELECT_VALUE: u32 = 2;

/// `color_primaries` / `transfer_characteristics` / `matrix_coefficients` "unspecified" value
/// (AV1 spec §6.4.2, `CP_UNSPECIFIED` / `TC_UNSPECIFIED` / `MC_UNSPECIFIED`).
const UNSPECIFIED: u32 = 2;
/// `CP_BT_709`.
const CP_BT_709: u32 = 1;
/// `TC_SRGB`.
const TC_SRGB: u32 = 13;
/// `MC_IDENTITY`.
const MC_IDENTITY: u32 = 0;

/// Parsed sequence-header fields this crate's VA-API decode parameter buffers and
/// [`super::frame_header`] need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each bool names one AV1 sequence_header_obu()/color_config() syntax element; a \
              state machine would obscure the 1:1 spec mapping this crate relies on for \
              review, same precedent as this crate's H.264 Pps"
)]
pub(super) struct SequenceHeader {
    /// `seq_profile` — always `0` (Main; this parser rejects anything else).
    pub(super) seq_profile: u8,
    /// `use_128x128_superblock` — needed by [`super::tile_info::parse`].
    pub(super) use_128x128_superblock: bool,
    /// `enable_order_hint`.
    pub(super) enable_order_hint: bool,
    /// `OrderHintBits` — `order_hint_bits_minus_1 + 1` when `enable_order_hint`, else `0`.
    pub(super) order_hint_bits: u32,
    /// `frame_width_bits_minus_1`.
    pub(super) frame_width_bits_minus_1: u32,
    /// `frame_height_bits_minus_1`.
    pub(super) frame_height_bits_minus_1: u32,
    /// `max_frame_width_minus_1`.
    pub(super) max_frame_width_minus_1: u32,
    /// `max_frame_height_minus_1`.
    pub(super) max_frame_height_minus_1: u32,
    /// `seq_force_screen_content_tools` (`0`, `1`, or `SELECT_SCREEN_CONTENT_TOOLS`).
    pub(super) seq_force_screen_content_tools: u32,
    /// `seq_force_integer_mv` (`0`, `1`, or `SELECT_INTEGER_MV`).
    pub(super) seq_force_integer_mv: u32,
    /// `color_config().color_range`.
    pub(super) color_range: bool,
    /// `color_config().matrix_coefficients` — forwarded verbatim to
    /// `PictureParameterBufferAV1::new`'s `matrix_coefficients` parameter.
    pub(super) matrix_coefficients: u8,
    /// `color_config().chroma_sample_position`.
    pub(super) chroma_sample_position: u8,
    /// `color_config().separate_uv_delta_q` — threads into
    /// [`super::frame_header`]'s `quantization_params()` (`diff_uv_delta`/`qm_v` presence).
    pub(super) separate_uv_delta_q: bool,
}

impl SequenceHeader {
    /// Parse a `sequence_header_obu()` payload (the OBU header/`leb128` size already stripped
    /// by [`super::obu::split_obus`]).
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::InvalidInput`] on truncated/malformed data, or
    /// [`DecodeError::Unsupported`] for anything outside this crate's scope (see the module
    /// doc).
    #[allow(
        clippy::too_many_lines,
        reason = "one linear, spec-section-ordered read sequence (sequence_header_obu() -> \
                  color_config()); splitting further would just move consecutive reads of the \
                  same syntax structure into a same-file helper with no independent reuse"
    )]
    pub(super) fn parse(data: &[u8]) -> Result<Self, DecodeError> {
        let mut r = BitReader::new(data);
        let map_err = |_| DecodeError::InvalidInput;

        let seq_profile = r.read_bits(3).map_err(map_err)?;
        if seq_profile != 0 {
            return Err(DecodeError::Unsupported);
        }
        let _still_picture = r.read_bit().map_err(map_err)? != 0;
        let reduced_still_picture_header = r.read_bit().map_err(map_err)? != 0;
        if reduced_still_picture_header {
            return Err(DecodeError::Unsupported);
        }

        let timing_info_present_flag = r.read_bit().map_err(map_err)? != 0;
        if timing_info_present_flag {
            // timing_info() / decoder_model_info() are not implemented this pass.
            return Err(DecodeError::Unsupported);
        }
        // timing_info_present_flag == 0 -> decoder_model_info_present_flag inferred 0, not
        // read; neither per-operating-point decoder-model field below is read either.

        let initial_display_delay_present_flag = r.read_bit().map_err(map_err)? != 0;
        if initial_display_delay_present_flag {
            return Err(DecodeError::Unsupported);
        }

        let operating_points_cnt_minus_1 = r.read_bits(5).map_err(map_err)?;
        if operating_points_cnt_minus_1 != 0 {
            // Single operating point only this pass.
            return Err(DecodeError::Unsupported);
        }
        let _operating_point_idc = r.read_bits(12).map_err(map_err)?;
        let seq_level_idx = r.read_bits(5).map_err(map_err)?;
        if seq_level_idx > 7 {
            let _seq_tier = r.read_bit().map_err(map_err)?;
        }

        let frame_width_bits_minus_1 = r.read_bits(4).map_err(map_err)?;
        let frame_height_bits_minus_1 = r.read_bits(4).map_err(map_err)?;
        let max_frame_width_minus_1 = r.read_bits(frame_width_bits_minus_1 + 1).map_err(map_err)?;
        let max_frame_height_minus_1 = r
            .read_bits(frame_height_bits_minus_1 + 1)
            .map_err(map_err)?;

        let frame_id_numbers_present_flag = r.read_bit().map_err(map_err)? != 0;
        if frame_id_numbers_present_flag {
            // cros-libva's PictureParameterBufferAV1 has no current_frame_id field — see
            // ADR-0003 § VA-API-specific plumbing.
            return Err(DecodeError::Unsupported);
        }

        let use_128x128_superblock = r.read_bit().map_err(map_err)? != 0;
        if r.read_bit().map_err(map_err)? != 0 {
            return Err(DecodeError::Unsupported); // enable_filter_intra
        }
        if r.read_bit().map_err(map_err)? != 0 {
            return Err(DecodeError::Unsupported); // enable_intra_edge_filter
        }
        if r.read_bit().map_err(map_err)? != 0 {
            return Err(DecodeError::Unsupported); // enable_interintra_compound
        }
        if r.read_bit().map_err(map_err)? != 0 {
            return Err(DecodeError::Unsupported); // enable_masked_compound
        }
        if r.read_bit().map_err(map_err)? != 0 {
            return Err(DecodeError::Unsupported); // enable_warped_motion
        }
        if r.read_bit().map_err(map_err)? != 0 {
            return Err(DecodeError::Unsupported); // enable_dual_filter
        }

        let enable_order_hint = r.read_bit().map_err(map_err)? != 0;
        if enable_order_hint {
            if r.read_bit().map_err(map_err)? != 0 {
                return Err(DecodeError::Unsupported); // enable_jnt_comp
            }
            if r.read_bit().map_err(map_err)? != 0 {
                return Err(DecodeError::Unsupported); // enable_ref_frame_mvs
            }
        }

        let seq_choose_screen_content_tools = r.read_bit().map_err(map_err)? != 0;
        let seq_force_screen_content_tools = if seq_choose_screen_content_tools {
            SELECT_VALUE
        } else {
            r.read_bit().map_err(map_err)?
        };
        let seq_force_integer_mv = if seq_force_screen_content_tools > 0 {
            let seq_choose_integer_mv = r.read_bit().map_err(map_err)? != 0;
            if seq_choose_integer_mv {
                SELECT_VALUE
            } else {
                r.read_bit().map_err(map_err)?
            }
        } else {
            SELECT_VALUE
        };
        let order_hint_bits = if enable_order_hint {
            r.read_bits(3).map_err(map_err)? + 1
        } else {
            0
        };

        if r.read_bit().map_err(map_err)? != 0 {
            return Err(DecodeError::Unsupported); // enable_superres
        }
        if r.read_bit().map_err(map_err)? != 0 {
            return Err(DecodeError::Unsupported); // enable_cdef
        }
        if r.read_bit().map_err(map_err)? != 0 {
            return Err(DecodeError::Unsupported); // enable_restoration
        }

        // color_config() (AV1 spec §5.5.2).
        let high_bitdepth = r.read_bit().map_err(map_err)? != 0;
        if high_bitdepth {
            return Err(DecodeError::Unsupported);
        }
        // seq_profile == 0 (enforced above) -> mono_chrome is always read (only seq_profile
        // == 1 skips it).
        let mono_chrome = r.read_bit().map_err(map_err)? != 0;
        if mono_chrome {
            return Err(DecodeError::Unsupported);
        }
        let color_description_present_flag = r.read_bit().map_err(map_err)? != 0;
        let (color_primaries, transfer_characteristics, matrix_coefficients) =
            if color_description_present_flag {
                (
                    r.read_bits(8).map_err(map_err)?,
                    r.read_bits(8).map_err(map_err)?,
                    r.read_bits(8).map_err(map_err)?,
                )
            } else {
                (UNSPECIFIED, UNSPECIFIED, UNSPECIFIED)
            };

        let (color_range, chroma_sample_position, separate_uv_delta_q) = if color_primaries
            == CP_BT_709
            && transfer_characteristics == TC_SRGB
            && matrix_coefficients == MC_IDENTITY
        {
            // mono_chrome == 0 rejected above, so this branch's own mono_chrome-only
            // early return never applies here.
            let separate_uv_delta_q = r.read_bit().map_err(map_err)? != 0;
            (true, 0u8, separate_uv_delta_q)
        } else {
            let color_range = r.read_bit().map_err(map_err)? != 0;
            // seq_profile == 0 -> subsampling_x = subsampling_y = 1 (4:2:0), not read;
            // both true -> chroma_sample_position is read unconditionally here.
            let chroma_sample_position =
                u8::try_from(r.read_bits(2).map_err(map_err)?).unwrap_or(0);
            let separate_uv_delta_q = r.read_bit().map_err(map_err)? != 0;
            (color_range, chroma_sample_position, separate_uv_delta_q)
        };

        let film_grain_params_present = r.read_bit().map_err(map_err)? != 0;
        if film_grain_params_present {
            return Err(DecodeError::Unsupported);
        }

        Ok(Self {
            seq_profile: 0,
            use_128x128_superblock,
            enable_order_hint,
            order_hint_bits,
            frame_width_bits_minus_1,
            frame_height_bits_minus_1,
            max_frame_width_minus_1,
            max_frame_height_minus_1,
            seq_force_screen_content_tools,
            seq_force_integer_mv,
            color_range,
            matrix_coefficients: u8::try_from(matrix_coefficients).unwrap_or(0),
            chroma_sample_position,
            separate_uv_delta_q,
        })
    }

    /// Coded picture width in luma samples (`max_frame_width_minus_1 + 1`).
    #[must_use]
    pub(super) const fn width(&self) -> u32 {
        self.max_frame_width_minus_1 + 1
    }

    /// Coded picture height in luma samples (`max_frame_height_minus_1 + 1`).
    #[must_use]
    pub(super) const fn height(&self) -> u32 {
        self.max_frame_height_minus_1 + 1
    }
}

#[cfg(test)]
#[path = "sequence_header_tests.rs"]
mod tests;
