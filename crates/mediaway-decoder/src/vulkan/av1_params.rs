//! AV1 OBU scanning + sequence-header parsing, and `StdVideoAV1SequenceHeader`
//! construction — this crate's first AV1 bitstream parser.
//!
//! No reusable AV1 parsing code exists elsewhere in this workspace: `mediaway_sw::av1` is a
//! `rav1e` **encoder**, not a parser (see `adr/vulkan/0002`'s "Bitstream-
//! parser reuse" section), and `mediaway-encoder::vulkan::nal`'s
//! `scan_obu_headers`/`read_leb128` are a `#[cfg(test)]`-gated private test
//! helper of a *different* crate — not imported here, matching
//! `adr/vulkan/0001`'s "duplicated, not imported" convention for cross-crate
//! bitstream helpers (confirmed via Grep this session: that helper is test-only).
//!
//! **Scope** (`adr/vulkan/0002`'s § Scope decision, KEY_FRAME-only first
//! increment): `frame_type == KEY_FRAME`, `show_frame == 1`, single tile
//! (`TileCols == TileRows == 1`), Main profile (`seq_profile == 0`), 8-bit
//! 4:2:0 only, no film grain (`film_grain_params_present == 0`), no frame-id
//! numbering, no super-resolution. Anything outside this scope is rejected
//! with [`Av1ParamError::Unsupported`], mirroring `hevc_params.rs`'s identical
//! "reject rather than silently mis-parse" convention. The real
//! `frame_type == KEY_FRAME` uncompressed-header parse (segmentation/
//! quantization/loop-filter/CDEF/loop-restoration/tile-info) lives in the
//! sibling [`av1_frame_header`] submodule — split out to stay under this
//! workspace's 1000-line-per-source-file rule, mirroring `hevc_params.rs`'s
//! own `hevc_ptl` submodule split.
//!
//! AV1 has no Annex-B-style start codes; OBUs use their own
//! `obu_header()`/`leb128`-coded `obu_size` framing (AV1 spec § 5.2/§ 5.3).
//! [`scan_obus`]/[`read_leb128`] are this crate's own real (non-test-only)
//! scanner.
//!
//! **`StdVideoDecodeAV1PictureInfo`/`StdVideoDecodeAV1ReferenceInfo`/
//! `VkVideoDecodeAV1*KHR` field names below are taken verbatim from
//! `adr/vulkan/0002`'s 2026-08-19 addendum**, confirmed there against the
//! real vendored `vulkanalia-sys` 0.35.0 source (`video.rs`/`structs.rs`) —
//! re-confirmed directly again this implementation pass by reading that same
//! vendored source. Both `StdVideoDecodeAV1*` structs mix `snake_case` and
//! `PascalCase`/`camelCase` field names in the same struct (bindgen preserved
//! the C header's own inconsistent naming) — every read/write site of those
//! exact field names in this crate carries its own item-scoped
//! `#[allow(non_snake_case)]`, not a blanket crate-wide allow.

#![forbid(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "every count here comes from a bounded AV1 bitstream field (spec-fixed field widths, \
              or this crate's own small fixed constants), always small in practice — narrowing \
              casts into the small Std*/vulkanalia field widths mirror this crate's own \
              hevc_params.rs allow for the identical shape"
)]

use mediaway_sw::h264::{BitReader, H264Error};
use thiserror::Error;
use vulkanalia::vk::video as native;

mod av1_frame_header;
pub(crate) use av1_frame_header::{Av1FrameHeader, Av1PictureInfoOptionals, parse_frame_header};

/// Errors from parsing an AV1 OBU / sequence header / `KEY_FRAME`
/// uncompressed header, or a syntax element this crate's scope does not
/// support.
///
/// Crate-internal, mirrors `hevc_params::HevcParamError`'s role — wrapped
/// into [`crate::vulkan::session::VulkanDecodeError`].
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Av1ParamError {
    /// Truncated data or a bit-reader overflow while reading (the same
    /// generic bit-reader error type `hevc_params.rs` reuses for its own
    /// non-H.264 bitstreams — it names no H.264-specific concept).
    #[error(transparent)]
    Bitstream(#[from] H264Error),
    /// A syntax element this crate's `KEY_FRAME`-only scope does not decode
    /// (see the module doc's scope list).
    #[error("unsupported AV1 syntax: {reason}")]
    Unsupported {
        /// Human-readable reason, always a `'static` literal at call sites.
        reason: &'static str,
    },
}

/// AV1 OBU type (`obu_type`, AV1 spec § 6.2.2) — only the values this
/// crate's decode path checks get named variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ObuType {
    /// `OBU_SEQUENCE_HEADER` (1).
    SequenceHeader,
    /// `OBU_TEMPORAL_DELIMITER` (2).
    TemporalDelimiter,
    /// `OBU_FRAME_HEADER` (3) — a standalone frame header, no tile-group
    /// payload (used when a stream splits frame header and tile data into
    /// separate OBUs). Not produced by `rav1e` for a single-tile frame (see
    /// the crate-level test fixture bytes this module's tests hardcode), but
    /// named for completeness/future general-GOP work.
    FrameHeader,
    /// `OBU_TILE_GROUP` (4) — sibling of [`ObuType::FrameHeader`].
    TileGroup,
    /// `OBU_METADATA` (5).
    Metadata,
    /// `OBU_FRAME` (6) — frame header **and** tile group combined in one OBU;
    /// the shape `rav1e` actually emits for a single-tile frame (confirmed by
    /// instrumented byte inspection this implementation pass, per
    /// `adr/vulkan/0002`'s own open question #5).
    Frame,
    /// `OBU_REDUNDANT_FRAME_HEADER` (7).
    RedundantFrameHeader,
    /// `OBU_TILE_LIST` (8).
    TileList,
    /// `OBU_PADDING` (15).
    Padding,
    /// Any other type value (reserved/extension types).
    Other(u8),
}

impl ObuType {
    const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::SequenceHeader,
            2 => Self::TemporalDelimiter,
            3 => Self::FrameHeader,
            4 => Self::TileGroup,
            5 => Self::Metadata,
            6 => Self::Frame,
            7 => Self::RedundantFrameHeader,
            8 => Self::TileList,
            15 => Self::Padding,
            other => Self::Other(other),
        }
    }
}

/// One parsed OBU: type + payload bytes (header/size field already
/// stripped).
#[derive(Debug, Clone, Copy)]
pub struct Obu<'a> {
    /// Decoded `obu_type`.
    pub obu_type: ObuType,
    /// Payload bytes after the header (and, for an extension header, after
    /// that byte too) and the `leb128` size field, if present.
    pub payload: &'a [u8],
}

/// Reads an AV1 `leb128()`-coded unsigned integer (AV1 spec § 4.10.5) from
/// the start of `data`.
///
/// # Errors
///
/// [`Av1ParamError::Bitstream`] if `data` is exhausted before a terminating
/// byte (high bit clear) is found; [`Av1ParamError::Unsupported`] if the
/// value would need more than 8 `leb128` bytes (AV1 spec's own `leb128()`
/// syntax caps this at 8).
pub(crate) fn read_leb128(data: &[u8]) -> Result<(u64, usize), Av1ParamError> {
    let mut value: u64 = 0;
    for i in 0..8usize {
        let byte = *data.get(i).ok_or(H264Error::UnexpectedEof)?;
        value |= u64::from(byte & 0x7f) << (i * 7);
        if byte & 0x80 == 0 {
            return Ok((value, i + 1));
        }
    }
    Err(Av1ParamError::Unsupported {
        reason: "leb128 value exceeds the AV1 spec's own 8-byte cap",
    })
}

/// Scans `data` (one packet's worth of "low overhead bitstream format" OBUs,
/// AV1 spec § 5.2 — no Annex-B start codes, see the module doc) into a list
/// of [`Obu`]s.
///
/// An OBU without `obu_has_size_field` set is accepted only as the **last**
/// OBU in `data` (its payload is "the rest of the buffer," AV1 spec's own
/// rule for a temporal unit's final OBU) — any earlier such OBU is rejected
/// with [`Av1ParamError::Unsupported`], since this crate has no outer
/// container framing to bound it otherwise.
///
/// # Errors
///
/// [`Av1ParamError::Bitstream`] on truncated data; [`Av1ParamError::Unsupported`]
/// on a nonzero `obu_forbidden_bit`, a nonzero `obu_extension_flag` (temporal/
/// spatial-layer scalability is out of this crate's scope), or a
/// no-size-field OBU that is not the last one.
pub(crate) fn scan_obus(data: &[u8]) -> Result<Vec<Obu<'_>>, Av1ParamError> {
    let mut obus = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() {
        let header = *data.get(pos).ok_or(H264Error::UnexpectedEof)?;
        let forbidden = (header >> 7) & 1;
        if forbidden != 0 {
            return Err(Av1ParamError::Unsupported {
                reason: "obu_forbidden_bit != 0",
            });
        }
        let obu_type = ObuType::from_u8((header >> 3) & 0x0F);
        let extension_flag = (header >> 2) & 1 != 0;
        let has_size_field = (header >> 1) & 1 != 0;
        if extension_flag {
            return Err(Av1ParamError::Unsupported {
                reason: "obu_extension_flag != 0 (temporal/spatial scalability) is not supported",
            });
        }
        let mut cursor = pos.checked_add(1).ok_or(H264Error::UnexpectedEof)?;
        let payload_len = if has_size_field {
            let (size, leb_len) = read_leb128(data.get(cursor..).ok_or(H264Error::UnexpectedEof)?)?;
            cursor = cursor
                .checked_add(leb_len)
                .ok_or(H264Error::UnexpectedEof)?;
            usize::try_from(size).map_err(|_err| H264Error::UnexpectedEof)?
        } else {
            data.len()
                .checked_sub(cursor)
                .ok_or(H264Error::UnexpectedEof)?
        };
        let end = cursor
            .checked_add(payload_len)
            .ok_or(H264Error::UnexpectedEof)?;
        let payload = data.get(cursor..end).ok_or(H264Error::UnexpectedEof)?;
        obus.push(Obu { obu_type, payload });
        pos = end;
    }
    Ok(obus)
}

/// `SELECT_SCREEN_CONTENT_TOOLS`/`SELECT_INTEGER_MV` (AV1 spec constant,
/// value `2`) — a sequence header signaling this value means "decided
/// per-frame," not a fixed sequence-wide choice.
const SELECT_VALUE: u8 = 2;

/// Parsed AV1 sequence header fields this crate's decode session needs.
///
/// Deliberately narrower than the full `sequence_header_obu()` syntax:
/// per-operating-point fields (`operating_point_idc`/`seq_level_idx`/
/// `seq_tier`/decoder-model timing) are parsed (to stay bit-aligned) and
/// discarded — `StdVideoAV1SequenceHeader` has no per-operating-point array
/// field for a decoder to populate (unlike the encode-side
/// `StdVideoEncodeAV1OperatingPointInfo`, a separate struct this crate's
/// decode path never builds).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each bool is a real, independent AV1 sequence-header flag that must be echoed into \
              StdVideoAV1SequenceHeader/StdVideoAV1ColorConfig exactly as signaled — same \
              reasoning as HevcSps's identical allow"
)]
pub struct Av1SequenceHeader {
    /// `seq_profile` — this crate rejects anything but `0` (Main), matching
    /// [`crate::vulkan::session::DecodeProfile::new_av1`]'s requested
    /// profile.
    pub seq_profile: u8,
    pub(crate) frame_width_bits_minus_1: u8,
    pub(crate) frame_height_bits_minus_1: u8,
    pub(crate) max_frame_width_minus_1: u16,
    pub(crate) max_frame_height_minus_1: u16,
    pub(crate) use_128x128_superblock: bool,
    pub(crate) enable_filter_intra: bool,
    pub(crate) enable_intra_edge_filter: bool,
    pub(crate) enable_interintra_compound: bool,
    pub(crate) enable_masked_compound: bool,
    pub(crate) enable_warped_motion: bool,
    pub(crate) enable_dual_filter: bool,
    pub(crate) enable_order_hint: bool,
    pub(crate) enable_jnt_comp: bool,
    pub(crate) enable_ref_frame_mvs: bool,
    pub(crate) seq_force_screen_content_tools: u8,
    pub(crate) seq_force_integer_mv: u8,
    /// `order_hint_bits_minus_1 + 1`, or `0` if `enable_order_hint == 0`.
    pub(crate) order_hint_bits: u32,
    pub(crate) enable_superres: bool,
    pub(crate) enable_cdef: bool,
    pub(crate) enable_restoration: bool,
    /// Always `false` for a sequence header this crate accepts — see
    /// [`Av1SequenceHeader::parse`]'s rejection of `film_grain_params_present
    /// == 1` (architecturally excluded, `adr/vulkan/0002`'s § Scope decision
    /// reason 3: forces `DISTINCT` DPB/output mode, incompatible with this
    /// crate's `COINCIDE`-only image design).
    pub(crate) film_grain_params_present: bool,
    pub(crate) subsampling_x: u8,
    pub(crate) subsampling_y: u8,
    pub(crate) separate_uv_delta_q: bool,
    pub(crate) color_range: bool,
}

impl Av1SequenceHeader {
    /// Parse a `sequence_header_obu()` payload (AV1 spec § 5.5.1).
    ///
    /// # Errors
    ///
    /// [`Av1ParamError::Unsupported`] when `seq_profile != 0` (Main only),
    /// `reduced_still_picture_header == 1` (this crate always parses the full
    /// sequence-header shape — a real encoder's simplest single-frame output,
    /// per this implementation pass's own `rav1e` byte inspection, already
    /// uses the full shape, not the reduced one), `frame_id_numbers_present_flag
    /// == 1`, `mono_chrome == 1`, chroma subsampling other than 4:2:0,
    /// `high_bitdepth == 1` (8-bit only), or `film_grain_params_present == 1`.
    /// Other [`Av1ParamError::Bitstream`] variants on truncated/overflowing
    /// data.
    #[allow(
        clippy::too_many_lines,
        reason = "linear AV1 spec § 5.5.1 sequence_header_obu() syntax-element sequence through \
                  color_config() — splitting further would just move consecutive reads of the \
                  same OBU payload into a same-file helper"
    )]
    pub fn parse(payload: &[u8]) -> Result<Self, Av1ParamError> {
        let mut reader = BitReader::new(payload);
        let seq_profile = reader.read_bits(3)? as u8;
        if seq_profile != 0 {
            return Err(Av1ParamError::Unsupported {
                reason: "seq_profile != 0 (Main profile only) is not supported",
            });
        }
        let still_picture = reader.read_bit()? != 0;
        let _ = still_picture;
        let reduced_still_picture_header = reader.read_bit()? != 0;
        if reduced_still_picture_header {
            return Err(Av1ParamError::Unsupported {
                reason: "reduced_still_picture_header == 1 is not supported this round",
            });
        }

        let timing_info_present_flag = reader.read_bit()? != 0;
        let mut decoder_model_info_present_flag = false;
        if timing_info_present_flag {
            // timing_info(): num_units_in_display_tick f(32), time_scale f(32),
            // equal_picture_interval f(1) [+ num_ticks_per_picture_minus_1 uvlc()
            // if set] — this crate's decode path never reads timing values back,
            // so only bit-position bookkeeping matters here.
            let _num_units_in_display_tick = reader.read_bits(32)?;
            let _time_scale = reader.read_bits(32)?;
            let equal_picture_interval = reader.read_bit()? != 0;
            if equal_picture_interval {
                let _num_ticks_per_picture_minus_1 = read_uvlc(&mut reader)?;
            }
            decoder_model_info_present_flag = reader.read_bit()? != 0;
            if decoder_model_info_present_flag {
                // decoder_model_info(): three f(5)/f(32)-ish fixed-width fields.
                let _buffer_delay_length_minus_1 = reader.read_bits(5)?;
                let _num_units_in_decoding_tick = reader.read_bits(32)?;
                let _buffer_removal_time_length_minus_1 = reader.read_bits(5)?;
                let _frame_presentation_time_length_minus_1 = reader.read_bits(5)?;
            }
        }
        let initial_display_delay_present_flag = reader.read_bit()? != 0;
        let operating_points_cnt_minus_1 = reader.read_bits(5)?;
        for _ in 0..=operating_points_cnt_minus_1 {
            let _operating_point_idc = reader.read_bits(12)?;
            let seq_level_idx = reader.read_bits(5)?;
            if seq_level_idx > 7 {
                let _seq_tier = reader.read_bit()?;
            }
            if decoder_model_info_present_flag {
                let decoder_model_present_for_this_op = reader.read_bit()? != 0;
                if decoder_model_present_for_this_op {
                    // operating_parameters_info(): two variable-width fields
                    // sized by decoder_model_info()'s own length fields, plus a
                    // fixed low_delay_mode_flag bit — this crate discards
                    // decoder_model_info_present_flag streams' exact field
                    // widths are already consumed generically above via
                    // decoder_model_info(); re-deriving the exact widths here
                    // would need carrying those lengths forward. Streams with
                    // decoder_model_info_present_flag == 1 are rare for a
                    // single-operating-point encoder and not exercised by this
                    // crate's own hardware-test bootstrap (`rav1e` never sets
                    // it — confirmed by this implementation pass's byte
                    // inspection) — rejected explicitly rather than silently
                    // mis-parsed.
                    return Err(Av1ParamError::Unsupported {
                        reason: "decoder_model_present_for_this_op == 1 is not supported this round",
                    });
                }
            }
            if initial_display_delay_present_flag {
                let initial_display_delay_present_for_this_op = reader.read_bit()? != 0;
                if initial_display_delay_present_for_this_op {
                    let _initial_display_delay_minus_1 = reader.read_bits(4)?;
                }
            }
        }

        let frame_width_bits_minus_1 = reader.read_bits(4)? as u8;
        let frame_height_bits_minus_1 = reader.read_bits(4)? as u8;
        let max_frame_width_minus_1 =
            reader.read_bits(u32::from(frame_width_bits_minus_1) + 1)? as u16;
        let max_frame_height_minus_1 =
            reader.read_bits(u32::from(frame_height_bits_minus_1) + 1)? as u16;

        let frame_id_numbers_present_flag = reader.read_bit()? != 0;
        if frame_id_numbers_present_flag {
            return Err(Av1ParamError::Unsupported {
                reason: "frame_id_numbers_present_flag == 1 is not supported this round",
            });
        }

        let use_128x128_superblock = reader.read_bit()? != 0;
        let enable_filter_intra = reader.read_bit()? != 0;
        let enable_intra_edge_filter = reader.read_bit()? != 0;
        let enable_interintra_compound = reader.read_bit()? != 0;
        let enable_masked_compound = reader.read_bit()? != 0;
        let enable_warped_motion = reader.read_bit()? != 0;
        let enable_dual_filter = reader.read_bit()? != 0;
        let enable_order_hint = reader.read_bit()? != 0;
        let (enable_jnt_comp, enable_ref_frame_mvs) = if enable_order_hint {
            (reader.read_bit()? != 0, reader.read_bit()? != 0)
        } else {
            (false, false)
        };
        let seq_choose_screen_content_tools = reader.read_bit()? != 0;
        let seq_force_screen_content_tools = if seq_choose_screen_content_tools {
            SELECT_VALUE
        } else {
            reader.read_bits(1)? as u8
        };
        let seq_force_integer_mv = if seq_force_screen_content_tools > 0 {
            let seq_choose_integer_mv = reader.read_bit()? != 0;
            if seq_choose_integer_mv {
                SELECT_VALUE
            } else {
                reader.read_bits(1)? as u8
            }
        } else {
            SELECT_VALUE
        };
        let order_hint_bits = if enable_order_hint {
            reader
                .read_bits(3)?
                .checked_add(1)
                .ok_or(H264Error::FieldOverflow)?
        } else {
            0
        };

        let enable_superres = reader.read_bit()? != 0;
        let enable_cdef = reader.read_bit()? != 0;
        let enable_restoration = reader.read_bit()? != 0;

        // color_config() — AV1 spec § 5.5.2.
        let high_bitdepth = reader.read_bit()? != 0;
        if high_bitdepth {
            return Err(Av1ParamError::Unsupported {
                reason: "high_bitdepth == 1 (10/12-bit) is not supported (8-bit only)",
            });
        }
        let mono_chrome = reader.read_bit()? != 0;
        if mono_chrome {
            return Err(Av1ParamError::Unsupported {
                reason: "mono_chrome == 1 is not supported (4:2:0 3-plane only)",
            });
        }
        let color_description_present_flag = reader.read_bit()? != 0;
        let (color_primaries, transfer_characteristics, matrix_coefficients) =
            if color_description_present_flag {
                (
                    reader.read_bits(8)? as u8,
                    reader.read_bits(8)? as u8,
                    reader.read_bits(8)? as u8,
                )
            } else {
                (2, 2, 2) // CP_UNSPECIFIED / TC_UNSPECIFIED / MC_UNSPECIFIED
            };
        // BT.709 sRGB identity-matrix shortcut (AV1 spec's own special case) —
        // not reachable when color_description_present_flag == 0 (UNSPECIFIED
        // never equals BT_709/SRGB/IDENTITY), so this crate's real streams
        // (none of which set color_description_present_flag, per this
        // implementation pass's byte inspection) always take the `else` branch
        // below.
        let is_identity_srgb =
            color_primaries == 1 && transfer_characteristics == 13 && matrix_coefficients == 0;
        let (color_range, subsampling_x, subsampling_y) = if is_identity_srgb {
            (true, 0u8, 0u8)
        } else {
            let color_range = reader.read_bit()? != 0;
            // seq_profile == 0 (checked above): subsampling is always 4:2:0.
            let subsampling_x = 1u8;
            let subsampling_y = 1u8;
            if subsampling_x == 1 && subsampling_y == 1 {
                let _chroma_sample_position = reader.read_bits(2)?;
            }
            (color_range, subsampling_x, subsampling_y)
        };
        let separate_uv_delta_q = reader.read_bit()? != 0;

        let film_grain_params_present = reader.read_bit()? != 0;
        if film_grain_params_present {
            return Err(Av1ParamError::Unsupported {
                reason: "film_grain_params_present == 1 is not supported (architecturally \
                          excluded — see adr/vulkan/0002 Scope decision reason 3)",
            });
        }

        Ok(Self {
            seq_profile,
            frame_width_bits_minus_1,
            frame_height_bits_minus_1,
            max_frame_width_minus_1,
            max_frame_height_minus_1,
            use_128x128_superblock,
            enable_filter_intra,
            enable_intra_edge_filter,
            enable_interintra_compound,
            enable_masked_compound,
            enable_warped_motion,
            enable_dual_filter,
            enable_order_hint,
            enable_jnt_comp,
            enable_ref_frame_mvs,
            seq_force_screen_content_tools,
            seq_force_integer_mv,
            order_hint_bits,
            enable_superres,
            enable_cdef,
            enable_restoration,
            film_grain_params_present,
            subsampling_x,
            subsampling_y,
            separate_uv_delta_q,
            color_range,
        })
    }

    /// Coded picture width in pixels (`max_frame_width_minus_1 + 1`) — this
    /// crate's `KEY_FRAME`-only scope always rejects `frame_size_override_flag
    /// == 1` combined with a smaller override (see
    /// [`av1_frame_header::Av1FrameHeader::parse`]), so the sequence header's
    /// own max dimensions are this session's real coded extent.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.max_frame_width_minus_1 as u32 + 1
    }

    /// Coded picture height in pixels — see [`Av1SequenceHeader::width`]'s
    /// doc.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.max_frame_height_minus_1 as u32 + 1
    }

    /// Builds the `StdVideoAV1ColorConfig` this sequence header's fields
    /// describe.
    #[must_use]
    pub fn to_std_color_config(&self) -> native::StdVideoAV1ColorConfig {
        let mut flags = native::StdVideoAV1ColorConfigFlags {
            _bitfield_align_1: [],
            _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
        };
        flags.set_mono_chrome(0);
        flags.set_color_range(u32::from(self.color_range));
        flags.set_separate_uv_delta_q(u32::from(self.separate_uv_delta_q));
        flags.set_color_description_present_flag(0);
        native::StdVideoAV1ColorConfig {
            flags,
            BitDepth: 8,
            subsampling_x: self.subsampling_x,
            subsampling_y: self.subsampling_y,
            reserved1: 0,
            color_primaries: native::STD_VIDEO_AV1_COLOR_PRIMARIES_UNSPECIFIED,
            transfer_characteristics: native::STD_VIDEO_AV1_TRANSFER_CHARACTERISTICS_UNSPECIFIED,
            matrix_coefficients: native::STD_VIDEO_AV1_MATRIX_COEFFICIENTS_UNSPECIFIED,
            chroma_sample_position: native::STD_VIDEO_AV1_CHROMA_SAMPLE_POSITION_UNKNOWN,
        }
    }

    /// All-zero `StdVideoAV1TimingInfo` — `pTimingInfo` must never be null
    /// (mirrors `mediaway-encoder::vulkan::av1_params::build_timing_info`'s
    /// identical "never null" fix), even though `timing_info_present_flag`
    /// is always `0` in the sequence header this crate builds (so its content
    /// is unused by the driver).
    #[must_use]
    pub const fn build_timing_info() -> native::StdVideoAV1TimingInfo {
        native::StdVideoAV1TimingInfo {
            flags: native::StdVideoAV1TimingInfoFlags {
                _bitfield_align_1: [],
                _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
            },
            num_units_in_display_tick: 0,
            time_scale: 0,
            num_ticks_per_picture_minus_1: 0,
        }
    }

    /// Builds the `StdVideoAV1SequenceHeader` this sequence header's fields
    /// describe. `color_config`/`timing_info` must outlive the returned
    /// struct (raw pointer fields — same pattern as `hevc_params.rs`'s
    /// `HevcSps::to_std`).
    #[must_use]
    #[allow(
        non_snake_case,
        reason = "StdVideoAV1SequenceHeaderFlags mixes snake_case and \
              camelCase field-setter names verbatim from the C header — see the module doc"
    )]
    pub fn to_std(
        &self,
        color_config: &native::StdVideoAV1ColorConfig,
        timing_info: &native::StdVideoAV1TimingInfo,
    ) -> native::StdVideoAV1SequenceHeader {
        let mut flags = native::StdVideoAV1SequenceHeaderFlags {
            _bitfield_align_1: [],
            _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
        };
        flags.set_still_picture(0);
        flags.set_reduced_still_picture_header(0);
        flags.set_use_128x128_superblock(u32::from(self.use_128x128_superblock));
        flags.set_enable_filter_intra(u32::from(self.enable_filter_intra));
        flags.set_enable_intra_edge_filter(u32::from(self.enable_intra_edge_filter));
        flags.set_enable_interintra_compound(u32::from(self.enable_interintra_compound));
        flags.set_enable_masked_compound(u32::from(self.enable_masked_compound));
        flags.set_enable_warped_motion(u32::from(self.enable_warped_motion));
        flags.set_enable_dual_filter(u32::from(self.enable_dual_filter));
        flags.set_enable_order_hint(u32::from(self.enable_order_hint));
        flags.set_enable_jnt_comp(u32::from(self.enable_jnt_comp));
        flags.set_enable_ref_frame_mvs(u32::from(self.enable_ref_frame_mvs));
        flags.set_frame_id_numbers_present_flag(0);
        flags.set_enable_superres(u32::from(self.enable_superres));
        flags.set_enable_cdef(u32::from(self.enable_cdef));
        flags.set_enable_restoration(u32::from(self.enable_restoration));
        flags.set_film_grain_params_present(0);
        flags.set_timing_info_present_flag(0);
        flags.set_initial_display_delay_present_flag(0);

        native::StdVideoAV1SequenceHeader {
            flags,
            seq_profile: native::STD_VIDEO_AV1_PROFILE_MAIN,
            frame_width_bits_minus_1: self.frame_width_bits_minus_1,
            frame_height_bits_minus_1: self.frame_height_bits_minus_1,
            max_frame_width_minus_1: self.max_frame_width_minus_1,
            max_frame_height_minus_1: self.max_frame_height_minus_1,
            delta_frame_id_length_minus_2: 0,
            additional_frame_id_length_minus_1: 0,
            order_hint_bits_minus_1: self.order_hint_bits.saturating_sub(1) as u8,
            seq_force_integer_mv: self.seq_force_integer_mv,
            seq_force_screen_content_tools: self.seq_force_screen_content_tools,
            reserved1: [0; 5],
            pColorConfig: color_config,
            pTimingInfo: timing_info,
        }
    }
}

/// Reads an AV1 `uvlc()` (variable-length unsigned Exp-Golomb-style code,
/// AV1 spec § 4.10.3) — distinct from H.264's `ue(v)` bit layout
/// (H.264 reads the leading-zero prefix then the suffix as one MSB-first
/// field; AV1's `uvlc()` is defined identically in practice for this crate's
/// purposes, but implemented locally rather than reusing
/// [`BitReader::read_ue`] to keep this module's AV1-spec-section citations
/// self-contained).
fn read_uvlc(reader: &mut BitReader<'_>) -> Result<u32, Av1ParamError> {
    let mut leading_zeros = 0u32;
    loop {
        let done = reader.read_bit()? != 0;
        if done {
            break;
        }
        leading_zeros += 1;
        if leading_zeros >= 32 {
            return Err(Av1ParamError::Unsupported {
                reason: "uvlc() leading-zero prefix overflow",
            });
        }
    }
    if leading_zeros >= 32 {
        return Ok(u32::MAX);
    }
    let value = reader.read_bits(leading_zeros)?;
    Ok(value + (1u32 << leading_zeros) - 1)
}

#[cfg(test)]
#[path = "av1_params_tests.rs"]
mod tests;
