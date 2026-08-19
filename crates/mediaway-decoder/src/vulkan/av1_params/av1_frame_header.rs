//! `KEY_FRAME`-only `uncompressed_header()` parsing (AV1 spec § 5.9.2) and
//! `StdVideoDecodeAV1PictureInfo`/`StdVideoAV1*` construction — split out of
//! `av1_params.rs` to stay under this workspace's 1000-line-per-source-file
//! rule, mirroring `hevc_params.rs`'s own `hevc_ptl` submodule split (see
//! that file's module doc).
//!
//! Every syntax element this crate's `KEY_FRAME`/`show_frame == 1`/
//! `FrameIsIntra`/single-tile scope actually reads from the bitstream is
//! parsed for real (segmentation, quantization, loop filter, CDEF, loop
//! restoration, tile info) — this is **not** a stub. Syntax elements that AV1
//! spec's own `uncompressed_header()` control flow never reaches for an
//! intra picture (motion-vector precision, interpolation filter, warped
//! motion, reference-frame signaling, global motion) are not read — the spec
//! itself skips them for `FrameIsIntra == 1`, not this crate's own choice —
//! and the corresponding `StdVideoDecodeAV1PictureInfo` fields are set to the
//! same fixed values `mediaway-encoder::vulkan::av1_params`'s always-`KEY_FRAME`
//! encode side already uses for the identical reason (unused by intra
//! decode/encode either way).
//!
//! **Bitstream-framing / `frameHeaderOffset` design decision** (resolves
//! `adr/vulkan/0002`'s open question #2 for this crate's single-tile scope):
//! this crate uploads **only the `OBU_FRAME`'s payload bytes** (after the
//! `obu_header()`/`leb128` size field, i.e. exactly `frame_header_obu()` +
//! `byte_alignment()` + `tile_group_obu()`) as `VkVideoDecodeInfoKHR::src_buffer`,
//! unlike H.264/HEVC's "prepend a start code, point offsets at byte 0 of the
//! whole thing" convention (AV1 has no equivalent outer framing byte to
//! include). `frameHeaderOffset` is therefore always `0` (the frame header
//! begins at the start of the uploaded range), and the single tile's
//! `pTileOffsets`/`pTileSizes` entry is computed from this parser's own
//! [`BitReader::bits_read`] position at the end of `uncompressed_header()`,
//! rounded up to a byte boundary (`byte_alignment()`) — `tile_group_obu()`'s
//! own leading `tile_start_and_end_present_flag`/`byte_alignment()` are both
//! no-ops in the single-tile case (AV1 spec § 5.11.1: `tile_start_and_end_present_flag`
//! is only read when `NumTiles > 1`), so no further bits separate the two.
//! This is a real design decision made without a cross-check against a known-
//! working reference decoder (unlike H.264/HEVC's offset conventions, which
//! were confirmed against `FFmpeg`'s `vulkan_decode.c` — no equivalent AV1
//! Vulkan decode reference was available to this implementation pass) —
//! flagged here for whoever picks up a genuine driver-level bug report
//! against it.

#![forbid(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "every count here comes from a bounded AV1 bitstream field (spec-fixed field widths) \
              or this crate's own small fixed constants — mirrors av1_params.rs's identical allow"
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "workspace `unreachable_pub` policy (Cargo.toml) wants `pub(crate)` here; \
              clippy::pedantic's redundant_pub_crate disagrees for private modules — the \
              two lints are mutually exclusive for this shape, workspace policy wins"
)]

use mediaway_sw::h264::{BitReader, H264Error};

use super::{Av1ParamError, Av1SequenceHeader};

/// AV1 spec Table (§ 5.9.14) `Segmentation_Feature_Bits`/`_Signed`/`_Max` —
/// `SEG_LVL_ALT_Q`, `_ALT_LF_Y_V`, `_ALT_LF_Y_H`, `_ALT_LF_U`, `_ALT_LF_V`,
/// `_REF_FRAME`, `_SKIP`, `_GLOBALMV`, in that order.
const SEG_FEATURE_BITS: [u32; 8] = [8, 6, 6, 6, 6, 3, 0, 0];
const SEG_FEATURE_SIGNED: [bool; 8] = [true, true, true, true, true, false, false, false];
const SEG_FEATURE_MAX: [i32; 8] = [255, 63, 63, 63, 63, 7, 0, 0];

/// Reads a signed `su(n)` field (AV1 spec § 4.10.6: an `f(n)`-read unsigned
/// value whose MSB is a sign bit).
fn read_su(reader: &mut BitReader<'_>, n: u32) -> Result<i32, Av1ParamError> {
    let value = i32::try_from(reader.read_bits(n)?).map_err(|_err| H264Error::FieldOverflow)?;
    let sign_mask = 1i32 << (n - 1);
    Ok(if value & sign_mask != 0 {
        value - (sign_mask << 1)
    } else {
        value
    })
}

/// Reads a non-symmetric `ns(n)` field (AV1 spec § 4.10.7) — used only by
/// [`parse_tile_info`]'s non-uniform-tile-spacing branch.
fn read_ns(reader: &mut BitReader<'_>, n: u32) -> Result<u32, Av1ParamError> {
    if n <= 1 {
        return Ok(0);
    }
    let w = u32::BITS - (n - 1).leading_zeros();
    let m = (1u32 << w).wrapping_sub(n);
    let v = if w > 1 { reader.read_bits(w - 1)? } else { 0 };
    if v < m {
        return Ok(v);
    }
    let extra = reader.read_bit()?;
    Ok((v << 1).wrapping_sub(m).wrapping_add(extra))
}

/// `read_delta_q()` (AV1 spec § 5.9.12): `delta_coded f(1)`, then `delta_q
/// su(1+6)` if coded.
fn read_delta_q(reader: &mut BitReader<'_>) -> Result<i8, Av1ParamError> {
    if reader.read_bit()? == 0 {
        return Ok(0);
    }
    Ok(read_su(reader, 7)? as i8)
}

/// `tile_log2(blkSize, target)` (AV1 spec § 5.9.15): smallest `k` such that
/// `(blkSize << k) >= target`.
const fn tile_log2(blk_size: u64, target: u64) -> u32 {
    let mut k = 0u32;
    while (blk_size << k) < target {
        k += 1;
    }
    k
}

/// Parsed `tile_info()` result — this crate's scope requires exactly one
/// tile (`TileCols == TileRows == 1`, checked by the caller); `sb_cols`/
/// `sb_rows`/`mi_cols`/`mi_rows` are kept for [`Av1PictureInfoOptionals::new`]'s
/// `StdVideoAV1TileInfo` array construction.
struct TileInfoParsed {
    sb_cols: u32,
    sb_rows: u32,
}

/// `tile_info()` (AV1 spec § 5.9.15), full uniform/non-uniform-spacing logic
/// — real, spec-faithful, not a stub, since a real encoder's chosen bit
/// values (not just this crate's own assumptions) determine how many bits
/// this syntax element actually consumes.
#[allow(
    clippy::too_many_lines,
    reason = "linear AV1 spec § 5.9.15 tile_info() syntax-element sequence (uniform and \
              non-uniform branches) — splitting further would just move consecutive reads of the \
              same syntax element into a same-file helper"
)]
fn parse_tile_info(
    reader: &mut BitReader<'_>,
    use_128x128_superblock: bool,
    mi_cols: u32,
    mi_rows: u32,
) -> Result<TileInfoParsed, Av1ParamError> {
    let sb_shift = if use_128x128_superblock { 5 } else { 4 };
    let sb_size = sb_shift + 2;
    let sb_cols = if use_128x128_superblock {
        (mi_cols + 31) >> 5
    } else {
        (mi_cols + 15) >> 4
    };
    let sb_rows = if use_128x128_superblock {
        (mi_rows + 31) >> 5
    } else {
        (mi_rows + 15) >> 4
    };
    let max_tile_width_sb = 4096u64 >> sb_size;
    let max_tile_area_sb = (4096u64 * 2304) >> (2 * sb_size);
    let min_log2_tile_cols = tile_log2(max_tile_width_sb, u64::from(sb_cols));
    let max_log2_tile_cols = tile_log2(1, u64::from(sb_cols.min(64)));
    let max_log2_tile_rows = tile_log2(1, u64::from(sb_rows.min(64)));
    let min_log2_tiles = min_log2_tile_cols.max(tile_log2(
        max_tile_area_sb,
        u64::from(sb_rows) * u64::from(sb_cols),
    ));

    let uniform_tile_spacing_flag = reader.read_bit()? != 0;
    let (tile_cols_log2, tile_rows_log2) = if uniform_tile_spacing_flag {
        let mut cols_log2 = min_log2_tile_cols;
        while cols_log2 < max_log2_tile_cols {
            if reader.read_bit()? != 0 {
                cols_log2 += 1;
            } else {
                break;
            }
        }
        let min_log2_tile_rows = min_log2_tiles.saturating_sub(cols_log2);
        let mut rows_log2 = min_log2_tile_rows;
        while rows_log2 < max_log2_tile_rows {
            if reader.read_bit()? != 0 {
                rows_log2 += 1;
            } else {
                break;
            }
        }
        (cols_log2, rows_log2)
    } else {
        let mut widest_tile_sb = 0u32;
        let mut start_sb = 0u32;
        let mut tile_cols = 0u32;
        while start_sb < sb_cols {
            let max_width = (sb_cols - start_sb).min(max_tile_width_sb as u32);
            let width_in_sbs_minus_1 = read_ns(reader, max_width)?;
            let size_sb = width_in_sbs_minus_1 + 1;
            widest_tile_sb = widest_tile_sb.max(size_sb);
            start_sb += size_sb;
            tile_cols += 1;
            if tile_cols > 64 {
                return Err(Av1ParamError::Unsupported {
                    reason: "non-uniform tile_info() column count exceeds MAX_TILE_COLS",
                });
            }
        }
        let cols_log2 = tile_log2(1, u64::from(tile_cols));
        let area_sb = if min_log2_tiles > 0 {
            (u64::from(sb_rows) * u64::from(sb_cols)) >> (min_log2_tiles + 1)
        } else {
            u64::from(sb_rows) * u64::from(sb_cols)
        };
        let max_tile_height_sb = (area_sb / u64::from(widest_tile_sb.max(1))).max(1) as u32;
        let mut start_sb_row = 0u32;
        let mut tile_rows = 0u32;
        while start_sb_row < sb_rows {
            let max_height = (sb_rows - start_sb_row).min(max_tile_height_sb);
            let height_in_sbs_minus_1 = read_ns(reader, max_height)?;
            let size_sb = height_in_sbs_minus_1 + 1;
            start_sb_row += size_sb;
            tile_rows += 1;
            if tile_rows > 64 {
                return Err(Av1ParamError::Unsupported {
                    reason: "non-uniform tile_info() row count exceeds MAX_TILE_ROWS",
                });
            }
        }
        let rows_log2 = tile_log2(1, u64::from(tile_rows));
        (cols_log2, rows_log2)
    };

    if tile_cols_log2 > 0 || tile_rows_log2 > 0 {
        return Err(Av1ParamError::Unsupported {
            reason: "more than one AV1 tile is not supported this round (single-tile scope)",
        });
    }
    Ok(TileInfoParsed { sb_cols, sb_rows })
}

/// Parsed `KEY_FRAME` `uncompressed_header()` fields this crate's decode
/// session needs — see the module doc for what is/isn't read for an intra
/// picture.
#[derive(Debug, Clone, Copy)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each bool is a real, independent AV1 frame-header flag that must be echoed into \
              StdVideoDecodeAV1PictureInfo exactly as signaled — same reasoning as HevcSps's \
              identical allow"
)]
pub(crate) struct Av1FrameHeader {
    pub(crate) frame_width: u32,
    pub(crate) frame_height: u32,
    pub(crate) order_hint: u8,
    pub(crate) disable_cdf_update: bool,
    pub(crate) allow_screen_content_tools: bool,
    pub(crate) frame_size_override_flag: bool,
    pub(crate) render_and_frame_size_different: bool,
    pub(crate) allow_intrabc: bool,
    pub(crate) disable_frame_end_update_cdf: bool,
    pub(crate) base_q_idx: u8,
    pub(crate) delta_q_y_dc: i8,
    pub(crate) delta_q_u_dc: i8,
    pub(crate) delta_q_u_ac: i8,
    pub(crate) delta_q_v_dc: i8,
    pub(crate) delta_q_v_ac: i8,
    pub(crate) using_qmatrix: bool,
    pub(crate) qm_y: u8,
    pub(crate) qm_u: u8,
    pub(crate) qm_v: u8,
    pub(crate) segmentation_enabled: bool,
    pub(crate) segmentation_update_map: bool,
    pub(crate) segmentation_temporal_update: bool,
    pub(crate) segmentation_update_data: bool,
    pub(crate) feature_enabled: [[bool; 8]; 8],
    pub(crate) feature_data: [[i16; 8]; 8],
    pub(crate) delta_q_present: bool,
    pub(crate) delta_q_res: u8,
    pub(crate) delta_lf_present: bool,
    pub(crate) delta_lf_res: u8,
    pub(crate) delta_lf_multi: bool,
    pub(crate) loop_filter_level: [u8; 4],
    pub(crate) loop_filter_sharpness: u8,
    pub(crate) loop_filter_delta_enabled: bool,
    pub(crate) loop_filter_ref_deltas: [i8; 8],
    pub(crate) loop_filter_mode_deltas: [i8; 2],
    pub(crate) cdef_damping_minus_3: u8,
    pub(crate) cdef_bits: u8,
    pub(crate) cdef_y_pri_strength: [u8; 8],
    pub(crate) cdef_y_sec_strength: [u8; 8],
    pub(crate) cdef_uv_pri_strength: [u8; 8],
    pub(crate) cdef_uv_sec_strength: [u8; 8],
    pub(crate) frame_restoration_type: [u8; 3],
    pub(crate) loop_restoration_size: [u16; 3],
    pub(crate) uses_lr: bool,
    pub(crate) uses_chroma_lr: bool,
    /// `TxMode`-compatible small int: `0` = `ONLY_4X4` (`CodedLossless == 1`),
    /// `1` = `LARGEST`, `2` = `SELECT` — computed once in
    /// [`parse_frame_header`] since `coded_lossless` itself is transient
    /// (only `read_tx_mode()` needs it).
    pub(crate) tx_mode: u8,
    pub(crate) reduced_tx_set: bool,
    pub(crate) sb_cols: u32,
    pub(crate) sb_rows: u32,
    pub(crate) mi_cols: u32,
    pub(crate) mi_rows: u32,
}

/// Where the single tile's coded data begins/ends within the uploaded
/// `OBU_FRAME` payload — see the module doc's "Bitstream-framing" section.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TileLayout {
    /// Byte offset (from the start of the `OBU_FRAME` payload) where
    /// `tile_group_obu()`'s coded tile data begins — `frame_header_obu()`'s
    /// own `bits_read()` position, rounded up to a byte boundary.
    pub(crate) tile_offset: u32,
    /// Bytes remaining in the payload after `tile_offset` (the whole rest of
    /// the buffer, since `NumTiles == 1`).
    pub(crate) tile_size: u32,
}

/// Parses a `KEY_FRAME` `frame_header_obu()` (AV1 spec § 5.9.1/§ 5.9.2) from
/// an `OBU_FRAME`'s payload bytes, returning the parsed header plus where the
/// following `tile_group_obu()`'s coded data begins.
///
/// # Errors
///
/// [`Av1ParamError::Unsupported`] when `show_existing_frame == 1`,
/// `frame_type != KEY_FRAME`, `show_frame == 0`, `frame_size_override_flag ==
/// 1` together with an overridden size different from the sequence header's
/// max dimensions, `use_superres == 1`, or more than one AV1 tile (see
/// [`parse_tile_info`]). Other [`Av1ParamError::Bitstream`] variants on
/// truncated/overflowing data.
#[allow(
    clippy::too_many_lines,
    reason = "linear AV1 spec § 5.9.2 uncompressed_header() syntax-element sequence for the \
              FrameIsIntra branch — splitting further would just move consecutive reads of the \
              same frame header into a same-file helper, mirroring hevc_slice.rs's identical \
              precedent for HEVC's own slice-segment-header parse"
)]
#[allow(
    clippy::similar_names,
    reason = "delta_q_y_dc/delta_q_u_dc/delta_q_u_ac/delta_q_v_dc/delta_q_v_ac are the real AV1 \
              spec § 5.9.12 quantization_params() syntax element names (Y/U/V DC/AC delta-Q \
              terms) — matching, not confusable, names"
)]
pub(crate) fn parse_frame_header(
    payload: &[u8],
    seq: &Av1SequenceHeader,
) -> Result<(Av1FrameHeader, TileLayout), Av1ParamError> {
    const KEY_FRAME: u32 = 0;
    let mut reader = BitReader::new(payload);

    let show_existing_frame = reader.read_bit()? != 0;
    if show_existing_frame {
        return Err(Av1ParamError::Unsupported {
            reason: "show_existing_frame == 1 is not supported (no real decode call needed for \
                      it — see adr/vulkan/0002's own note; this crate's KEY_FRAME-only scope \
                      never needs to service it)",
        });
    }
    let frame_type = reader.read_bits(2)?;
    if frame_type != KEY_FRAME {
        return Err(Av1ParamError::Unsupported {
            reason: "frame_type != KEY_FRAME is not supported (KEY_FRAME-only scope)",
        });
    }
    let show_frame = reader.read_bit()? != 0;
    if !show_frame {
        return Err(Av1ParamError::Unsupported {
            reason: "show_frame == 0 is not supported this round",
        });
    }
    // frame_type == KEY_FRAME && show_frame == 1: error_resilient_mode = 1
    // (no bit read), showable_frame = false (computed, no bit read).

    let disable_cdf_update = reader.read_bit()? != 0;
    let allow_screen_content_tools = if seq.seq_force_screen_content_tools == super::SELECT_VALUE {
        reader.read_bit()? != 0
    } else {
        seq.seq_force_screen_content_tools != 0
    };
    // force_integer_mv: allow_screen_content_tools-gated bit, then forced to
    // 1 by FrameIsIntra regardless — the conditional bit still needs to be
    // read to stay bit-aligned, its value is simply overridden after.
    if allow_screen_content_tools && seq.seq_force_integer_mv == super::SELECT_VALUE {
        let _force_integer_mv = reader.read_bit()?;
    }
    // frame_id_numbers_present_flag == 0 (rejected in Av1SequenceHeader::parse
    // otherwise) — current_frame_id = 0, no bits.

    let frame_size_override_flag = reader.read_bit()? != 0;
    let order_hint = if seq.order_hint_bits > 0 {
        reader.read_bits(seq.order_hint_bits)? as u8
    } else {
        0
    };
    // FrameIsIntra: primary_ref_frame = PRIMARY_REF_NONE, no bit read.
    // decoder_model_info_present_flag is never true for a sequence header
    // this crate accepts (Av1SequenceHeader::parse rejects
    // decoder_model_present_for_this_op == 1, and a stream with
    // decoder_model_info_present_flag == 1 but no operating point using it
    // still never reaches this frame-header field) — no
    // buffer_removal_time_present_flag bit here.
    // frame_type == KEY_FRAME && show_frame == 1: refresh_frame_flags = 0xFF
    // (no bit read); the ref_order_hint refresh loop below is gated on
    // "!FrameIsIntra || refresh_frame_flags != allFrames", both false here.

    // frame_size() + render_size() (FrameIsIntra branch).
    let (frame_width, frame_height) = if frame_size_override_flag {
        let width = reader.read_bits(u32::from(seq.frame_width_bits_minus_1) + 1)? + 1;
        let height = reader.read_bits(u32::from(seq.frame_height_bits_minus_1) + 1)? + 1;
        if width != u32::from(seq.max_frame_width_minus_1) + 1
            || height != u32::from(seq.max_frame_height_minus_1) + 1
        {
            return Err(Av1ParamError::Unsupported {
                reason: "frame_size_override_flag == 1 with a size different from the sequence \
                          header's max dimensions is not supported this round",
            });
        }
        (width, height)
    } else {
        (
            u32::from(seq.max_frame_width_minus_1) + 1,
            u32::from(seq.max_frame_height_minus_1) + 1,
        )
    };
    let use_superres = if seq.enable_superres {
        reader.read_bit()? != 0
    } else {
        false
    };
    if use_superres {
        return Err(Av1ParamError::Unsupported {
            reason: "use_superres == 1 is not supported this round",
        });
    }
    let render_and_frame_size_different = reader.read_bit()? != 0;
    if render_and_frame_size_different {
        let _render_width_minus_1 = reader.read_bits(16)?;
        let _render_height_minus_1 = reader.read_bits(16)?;
    }
    let allow_intrabc = if allow_screen_content_tools {
        reader.read_bit()? != 0
    } else {
        false
    };

    let disable_frame_end_update_cdf = if disable_cdf_update {
        true
    } else {
        reader.read_bit()? != 0
    };
    // primary_ref_frame == PRIMARY_REF_NONE: init_non_coeff_cdfs()/
    // setup_past_independence() — no bits. use_ref_frame_mvs == 0 (intra) —
    // no motion_field_estimation().

    let mi_cols = 2 * ((frame_width + 7) >> 3);
    let mi_rows = 2 * ((frame_height + 7) >> 3);
    let tile_info = parse_tile_info(&mut reader, seq.use_128x128_superblock, mi_cols, mi_rows)?;

    let base_q_idx = reader.read_bits(8)? as u8;
    let delta_q_y_dc = read_delta_q(&mut reader)?;
    let diff_uv_delta = if seq.separate_uv_delta_q {
        reader.read_bit()? != 0
    } else {
        false
    };
    let delta_q_u_dc = read_delta_q(&mut reader)?;
    let delta_q_u_ac = read_delta_q(&mut reader)?;
    let (delta_q_v_dc, delta_q_v_ac) = if diff_uv_delta {
        (read_delta_q(&mut reader)?, read_delta_q(&mut reader)?)
    } else {
        (delta_q_u_dc, delta_q_u_ac)
    };
    let using_qmatrix = reader.read_bit()? != 0;
    let (qm_y, qm_u, qm_v) = if using_qmatrix {
        let qm_y = reader.read_bits(4)? as u8;
        let qm_u = reader.read_bits(4)? as u8;
        let qm_v = if seq.separate_uv_delta_q {
            reader.read_bits(4)? as u8
        } else {
            qm_u
        };
        (qm_y, qm_u, qm_v)
    } else {
        (0, 0, 0)
    };

    let (
        segmentation_enabled,
        segmentation_update_map,
        segmentation_temporal_update,
        segmentation_update_data,
        feature_enabled,
        feature_data,
    ) = parse_segmentation(&mut reader)?;

    let base_q_lossless = base_q_idx == 0
        && delta_q_y_dc == 0
        && delta_q_u_ac == 0
        && delta_q_u_dc == 0
        && delta_q_v_ac == 0
        && delta_q_v_dc == 0;
    let delta_q_present = if base_q_idx > 0 {
        reader.read_bit()? != 0
    } else {
        false
    };
    let delta_q_res = if delta_q_present {
        reader.read_bits(2)? as u8
    } else {
        0
    };
    let (delta_lf_present, delta_lf_res, delta_lf_multi) = if delta_q_present {
        let present = if allow_intrabc {
            false
        } else {
            reader.read_bit()? != 0
        };
        if present {
            (present, reader.read_bits(2)? as u8, reader.read_bit()? != 0)
        } else {
            (present, 0, false)
        }
    } else {
        (false, 0, false)
    };

    // CodedLossless (AV1 spec § 5.9.2): every segment's effective qindex is 0
    // and every delta-Q term is 0. get_qindex(1, segmentId) only differs from
    // base_q_idx when SEG_LVL_ALT_Q (index 0) is enabled for that segment.
    let coded_lossless = base_q_lossless
        && (0..8).all(|segment_id| {
            if segmentation_enabled && feature_enabled[segment_id][0] {
                let data = i32::from(feature_data[segment_id][0]);
                (i32::from(base_q_idx) + data).clamp(0, 255) == 0
            } else {
                base_q_idx == 0
            }
        });
    // AllLossless requires FrameWidth == UpscaledWidth, always true this
    // round (use_superres rejected above, so UpscaledWidth == FrameWidth).
    let all_lossless = coded_lossless;

    let (
        loop_filter_level,
        loop_filter_sharpness,
        loop_filter_delta_enabled,
        ref_deltas,
        mode_deltas,
    ) = parse_loop_filter(&mut reader, coded_lossless, allow_intrabc)?;
    let (cdef_damping_minus_3, cdef_bits, y_pri, y_sec, uv_pri, uv_sec) =
        parse_cdef(&mut reader, coded_lossless, allow_intrabc, seq.enable_cdef)?;
    let (frame_restoration_type, loop_restoration_size, uses_lr, uses_chroma_lr) = parse_lr(
        &mut reader,
        all_lossless,
        allow_intrabc,
        seq.enable_restoration,
        seq.use_128x128_superblock,
        seq.subsampling_x,
        seq.subsampling_y,
    )?;

    // read_tx_mode() (AV1 spec § 5.9.21): TxMode-compatible small int, see
    // Av1FrameHeader::tx_mode's own doc.
    let tx_mode: u8 = if coded_lossless {
        0
    } else if reader.read_bit()? != 0 {
        2
    } else {
        1
    };
    // frame_reference_mode(): FrameIsIntra -> reference_select = 0, no bit.
    // skip_mode_params(): FrameIsIntra -> skipModeAllowed = 0 -> no bit.
    // allow_warped_motion: FrameIsIntra -> 0, no bit.
    let reduced_tx_set = reader.read_bit()? != 0;
    // FrameIsIntra: no global_motion_params(). film_grain_params_present == 0
    // (rejected otherwise in Av1SequenceHeader::parse) -> film_grain_params()
    // resets to all-zero with no bits read.

    let header_bits = reader.bits_read();
    let header_bytes = header_bits.div_ceil(8);
    let header_bytes_u32 = u32::try_from(header_bytes).map_err(|_err| H264Error::FieldOverflow)?;
    let total_len = u32::try_from(payload.len()).map_err(|_err| H264Error::FieldOverflow)?;
    let tile_size = total_len
        .checked_sub(header_bytes_u32)
        .ok_or(H264Error::UnexpectedEof)?;
    if tile_size == 0 {
        return Err(H264Error::UnexpectedEof.into());
    }

    Ok((
        Av1FrameHeader {
            frame_width,
            frame_height,
            order_hint,
            disable_cdf_update,
            allow_screen_content_tools,
            frame_size_override_flag,
            render_and_frame_size_different,
            allow_intrabc,
            disable_frame_end_update_cdf,
            base_q_idx,
            delta_q_y_dc,
            delta_q_u_dc,
            delta_q_u_ac,
            delta_q_v_dc,
            delta_q_v_ac,
            using_qmatrix,
            qm_y,
            qm_u,
            qm_v,
            segmentation_enabled,
            segmentation_update_map,
            segmentation_temporal_update,
            segmentation_update_data,
            feature_enabled,
            feature_data,
            delta_q_present,
            delta_q_res,
            delta_lf_present,
            delta_lf_res,
            delta_lf_multi,
            loop_filter_level,
            loop_filter_sharpness,
            loop_filter_delta_enabled,
            loop_filter_ref_deltas: ref_deltas,
            loop_filter_mode_deltas: mode_deltas,
            cdef_damping_minus_3,
            cdef_bits,
            cdef_y_pri_strength: y_pri,
            cdef_y_sec_strength: y_sec,
            cdef_uv_pri_strength: uv_pri,
            cdef_uv_sec_strength: uv_sec,
            frame_restoration_type,
            loop_restoration_size,
            uses_lr,
            uses_chroma_lr,
            tx_mode,
            reduced_tx_set,
            sb_cols: tile_info.sb_cols,
            sb_rows: tile_info.sb_rows,
            mi_cols,
            mi_rows,
        },
        TileLayout {
            tile_offset: header_bytes_u32,
            tile_size,
        },
    ))
}

#[allow(
    clippy::type_complexity,
    reason = "one function's own tightly-related local return values, \
          not a reusable type worth naming separately"
)]
fn parse_segmentation(
    reader: &mut BitReader<'_>,
) -> Result<(bool, bool, bool, bool, [[bool; 8]; 8], [[i16; 8]; 8]), Av1ParamError> {
    let segmentation_enabled = reader.read_bit()? != 0;
    let mut feature_enabled = [[false; 8]; 8];
    let mut feature_data = [[0i16; 8]; 8];
    if !segmentation_enabled {
        return Ok((false, false, false, false, feature_enabled, feature_data));
    }
    // primary_ref_frame == PRIMARY_REF_NONE (this crate's KEY_FRAME-only
    // scope, always true): segmentation_update_map = 1,
    // segmentation_temporal_update = 0, segmentation_update_data = 1, no
    // bits read for any of the three.
    for (enabled_row, data_row) in feature_enabled.iter_mut().zip(feature_data.iter_mut()) {
        for j in 0..8usize {
            let enabled = reader.read_bit()? != 0;
            enabled_row[j] = enabled;
            if enabled {
                let bits = SEG_FEATURE_BITS[j];
                let limit = SEG_FEATURE_MAX[j];
                let value = if SEG_FEATURE_SIGNED[j] {
                    read_su(reader, 1 + bits)?
                } else {
                    i32::try_from(reader.read_bits(bits)?)
                        .map_err(|_err| H264Error::FieldOverflow)?
                };
                let lower = if SEG_FEATURE_SIGNED[j] { -limit } else { 0 };
                data_row[j] = value.clamp(lower, limit) as i16;
            }
        }
    }
    Ok((true, true, false, true, feature_enabled, feature_data))
}

#[allow(
    clippy::type_complexity,
    reason = "one function's own tightly-related local return values, mirrors this file's \
              identical parse_segmentation/parse_cdef/parse_lr allow"
)]
fn parse_loop_filter(
    reader: &mut BitReader<'_>,
    coded_lossless: bool,
    allow_intrabc: bool,
) -> Result<([u8; 4], u8, bool, [i8; 8], [i8; 2]), Av1ParamError> {
    let default_ref_deltas: [i8; 8] = [1, 0, 0, 0, -1, 0, -1, -1];
    if coded_lossless || allow_intrabc {
        return Ok(([0; 4], 0, false, default_ref_deltas, [0, 0]));
    }
    let mut level = [0u8; 4];
    level[0] = reader.read_bits(6)? as u8;
    level[1] = reader.read_bits(6)? as u8;
    // NumPlanes == 3 always this round (mono_chrome rejected in
    // Av1SequenceHeader::parse).
    if level[0] != 0 || level[1] != 0 {
        level[2] = reader.read_bits(6)? as u8;
        level[3] = reader.read_bits(6)? as u8;
    }
    let sharpness = reader.read_bits(3)? as u8;
    let delta_enabled = reader.read_bit()? != 0;
    let mut ref_deltas = default_ref_deltas;
    let mut mode_deltas = [0i8; 2];
    if delta_enabled {
        let delta_update = reader.read_bit()? != 0;
        if delta_update {
            for delta in &mut ref_deltas {
                if reader.read_bit()? != 0 {
                    *delta = read_su(reader, 7)? as i8;
                }
            }
            for delta in &mut mode_deltas {
                if reader.read_bit()? != 0 {
                    *delta = read_su(reader, 7)? as i8;
                }
            }
        }
    }
    Ok((level, sharpness, delta_enabled, ref_deltas, mode_deltas))
}

#[allow(
    clippy::type_complexity,
    reason = "one function's own tightly-related local return values"
)]
fn parse_cdef(
    reader: &mut BitReader<'_>,
    coded_lossless: bool,
    allow_intrabc: bool,
    enable_cdef: bool,
) -> Result<(u8, u8, [u8; 8], [u8; 8], [u8; 8], [u8; 8]), Av1ParamError> {
    if coded_lossless || allow_intrabc || !enable_cdef {
        return Ok((0, 0, [0; 8], [0; 8], [0; 8], [0; 8]));
    }
    let damping_minus_3 = reader.read_bits(2)? as u8;
    let bits = reader.read_bits(2)? as u8;
    let mut y_pri = [0u8; 8];
    let mut y_sec = [0u8; 8];
    let mut uv_pri = [0u8; 8];
    let mut uv_sec = [0u8; 8];
    let count = 1usize << bits;
    for entry in 0..count {
        y_pri[entry] = reader.read_bits(4)? as u8;
        let mut sec = reader.read_bits(2)? as u8;
        if sec == 3 {
            sec += 1;
        }
        y_sec[entry] = sec;
        // NumPlanes == 3 always this round.
        uv_pri[entry] = reader.read_bits(4)? as u8;
        let mut usec = reader.read_bits(2)? as u8;
        if usec == 3 {
            usec += 1;
        }
        uv_sec[entry] = usec;
    }
    Ok((damping_minus_3, bits, y_pri, y_sec, uv_pri, uv_sec))
}

/// `Remap_Lr_Type` (AV1 spec § 5.9.20): `lr_type` (2 bits) to
/// `StdVideoAV1FrameRestorationType`-compatible value.
const REMAP_LR_TYPE: [u8; 4] = [0, 3, 1, 2];
/// `RESTORATION_TILESIZE_MAX` (AV1 spec constant).
const RESTORATION_TILESIZE_MAX: u16 = 256;

#[allow(
    clippy::too_many_arguments,
    reason = "one real syntax element's full parameter set"
)]
#[allow(
    clippy::fn_params_excessive_bools,
    reason = "each bool is a real, independent AV1 spec § 5.9.20 lr_params() precondition/flag \
              (all_lossless/allow_intrabc/enable_restoration/use_128x128_superblock come from \
              the frame/sequence header this function reads, not a state-machine this crate \
              controls) — collapsing them into enums would obscure the 1:1 spec mapping"
)]
#[allow(
    clippy::type_complexity,
    reason = "one function's own tightly-related local return values, mirrors this file's \
              identical parse_segmentation/parse_cdef allow"
)]
fn parse_lr(
    reader: &mut BitReader<'_>,
    all_lossless: bool,
    allow_intrabc: bool,
    enable_restoration: bool,
    use_128x128_superblock: bool,
    subsampling_x: u8,
    subsampling_y: u8,
) -> Result<([u8; 3], [u16; 3], bool, bool), Av1ParamError> {
    if all_lossless || allow_intrabc || !enable_restoration {
        return Ok(([0; 3], [0; 3], false, false));
    }
    let mut frame_restoration_type = [0u8; 3];
    let mut uses_lr = false;
    let mut uses_chroma_lr = false;
    // NumPlanes == 3 always this round.
    for (plane, restoration_type) in frame_restoration_type.iter_mut().enumerate() {
        let lr_type = reader.read_bits(2)? as usize;
        let mapped = REMAP_LR_TYPE[lr_type];
        *restoration_type = mapped;
        if mapped != 0 {
            uses_lr = true;
            if plane > 0 {
                uses_chroma_lr = true;
            }
        }
    }
    let mut sizes = [0u16; 3];
    if uses_lr {
        let unit_shift: u16 = if use_128x128_superblock {
            u16::from(reader.read_bit()? != 0) + 1
        } else {
            let base = u16::from(reader.read_bit()? != 0);
            if base != 0 {
                base + u16::from(reader.read_bit()? != 0)
            } else {
                base
            }
        };
        sizes[0] = RESTORATION_TILESIZE_MAX >> (2 - unit_shift);
        let uv_shift = if subsampling_x == 1 && subsampling_y == 1 && uses_chroma_lr {
            u16::from(reader.read_bit()? != 0)
        } else {
            0
        };
        sizes[1] = sizes[0] >> uv_shift;
        sizes[2] = sizes[0] >> uv_shift;
    }
    Ok((frame_restoration_type, sizes, uses_lr, uses_chroma_lr))
}

mod av1_frame_std;
pub(crate) use av1_frame_std::Av1PictureInfoOptionals;

#[cfg(test)]
#[path = "av1_frame_header_tests.rs"]
mod tests;
