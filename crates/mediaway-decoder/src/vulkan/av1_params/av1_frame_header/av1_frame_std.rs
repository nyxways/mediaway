//! `StdVideoDecodeAV1PictureInfo`/`StdVideoDecodeAV1ReferenceInfo`/
//! `StdVideoAV1*` optional-struct construction from a parsed
//! [`super::Av1FrameHeader`] — split out of `av1_frame_header.rs` to stay
//! under this workspace's 1000-line-per-source-file rule (that file's own
//! parsing logic already fills it); a child module (not a sibling of
//! `av1_params.rs`) purely so it can reach `Av1FrameHeader`'s `pub`
//! fields via `super::`, mirroring how `hevc_ptl.rs` reaches into
//! `hevc_params.rs`.

#![forbid(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "mirrors av1_params.rs's/av1_frame_header.rs's identical allow — every value here is \
              a bounded AV1 syntax-element-derived count"
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "workspace `unreachable_pub` policy (Cargo.toml) wants `pub(crate)` here; \
              clippy::pedantic's redundant_pub_crate disagrees for private modules — the \
              two lints are mutually exclusive for this shape, workspace policy wins"
)]

use vulkanalia::vk::video as native;

use super::Av1FrameHeader;

/// Every owned `StdVideoAV1*` optional-pointer target
/// [`Av1FrameHeader::to_std_picture_info`] needs, bundled the same way
/// `mediaway-encoder::vulkan::av1_params::PictureInfoOptionals` bundles its
/// own encode-side equivalents — the caller (`session_command_av1.rs`) keeps
/// this alive on its own stack frame for the duration of one
/// `vkCmdDecodeVideoKHR` call.
pub(crate) struct Av1PictureInfoOptionals {
    pub(crate) quantization: native::StdVideoAV1Quantization,
    pub(crate) segmentation: native::StdVideoAV1Segmentation,
    pub(crate) loop_filter: native::StdVideoAV1LoopFilter,
    pub(crate) cdef: native::StdVideoAV1CDEF,
    pub(crate) loop_restoration: native::StdVideoAV1LoopRestoration,
    pub(crate) global_motion: native::StdVideoAV1GlobalMotion,
    pub(crate) tile_info: native::StdVideoAV1TileInfo,
    mi_col_starts: [u16; 2],
    mi_row_starts: [u16; 2],
    width_in_sbs_minus_1: [u16; 1],
    height_in_sbs_minus_1: [u16; 1],
}

impl Av1PictureInfoOptionals {
    /// Builds every optional struct from `header`'s real parsed fields (not
    /// all-disabled placeholders — unlike the encode side's always-disabled
    /// `PictureInfoOptionals::new`, this crate's decode input is a real
    /// encoder's output, which may legally enable segmentation/CDEF/loop
    /// filter/loop restoration for a `KEY_FRAME`, as this implementation
    /// pass's own `rav1e` test fixture bytes do for segmentation and CDEF).
    #[must_use]
    #[allow(
        clippy::too_many_lines,
        reason = "linear StdVideoAV1* optional-struct-by-struct construction from one parsed \
                  header — splitting further would just move consecutive struct builds into a \
                  same-file helper"
    )]
    pub(crate) fn new(header: &Av1FrameHeader) -> Self {
        let mut quantization_flags = native::StdVideoAV1QuantizationFlags {
            _bitfield_align_1: [],
            _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
        };
        quantization_flags.set_using_qmatrix(u32::from(header.using_qmatrix));
        quantization_flags.set_diff_uv_delta(u32::from(
            header.delta_q_v_dc != header.delta_q_u_dc
                || header.delta_q_v_ac != header.delta_q_u_ac,
        ));
        let quantization = native::StdVideoAV1Quantization {
            flags: quantization_flags,
            base_q_idx: header.base_q_idx,
            DeltaQYDc: header.delta_q_y_dc,
            DeltaQUDc: header.delta_q_u_dc,
            DeltaQUAc: header.delta_q_u_ac,
            DeltaQVDc: header.delta_q_v_dc,
            DeltaQVAc: header.delta_q_v_ac,
            qm_y: header.qm_y,
            qm_u: header.qm_u,
            qm_v: header.qm_v,
        };

        let feature_enabled_bytes: [u8; 8] = std::array::from_fn(|i| {
            header.feature_enabled[i]
                .iter()
                .enumerate()
                .fold(0u8, |acc, (bit, &enabled)| acc | (u8::from(enabled) << bit))
        });
        let segmentation = native::StdVideoAV1Segmentation {
            FeatureEnabled: feature_enabled_bytes,
            FeatureData: header.feature_data,
        };

        let mut loop_filter_flags = native::StdVideoAV1LoopFilterFlags {
            _bitfield_align_1: [],
            _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
        };
        loop_filter_flags
            .set_loop_filter_delta_enabled(u32::from(header.loop_filter_delta_enabled));
        loop_filter_flags.set_loop_filter_delta_update(u32::from(header.loop_filter_delta_enabled));
        let loop_filter = native::StdVideoAV1LoopFilter {
            flags: loop_filter_flags,
            loop_filter_level: header.loop_filter_level,
            loop_filter_sharpness: header.loop_filter_sharpness,
            update_ref_delta: u8::from(header.loop_filter_delta_enabled),
            loop_filter_ref_deltas: header.loop_filter_ref_deltas,
            update_mode_delta: u8::from(header.loop_filter_delta_enabled),
            loop_filter_mode_deltas: header.loop_filter_mode_deltas,
        };

        let cdef = native::StdVideoAV1CDEF {
            cdef_damping_minus_3: header.cdef_damping_minus_3,
            cdef_bits: header.cdef_bits,
            cdef_y_pri_strength: header.cdef_y_pri_strength,
            cdef_y_sec_strength: header.cdef_y_sec_strength,
            cdef_uv_pri_strength: header.cdef_uv_pri_strength,
            cdef_uv_sec_strength: header.cdef_uv_sec_strength,
        };

        let loop_restoration = native::StdVideoAV1LoopRestoration {
            FrameRestorationType: [
                restoration_type_from_u8(header.frame_restoration_type[0]),
                restoration_type_from_u8(header.frame_restoration_type[1]),
                restoration_type_from_u8(header.frame_restoration_type[2]),
            ],
            LoopRestorationSize: header.loop_restoration_size,
        };

        // No inter prediction this round (KEY_FRAME/FrameIsIntra never reads
        // global_motion_params()) — identity for every reference, matching
        // mediaway-encoder::vulkan::av1_params::build_global_motion.
        let global_motion = native::StdVideoAV1GlobalMotion {
            GmType: [0; 8],
            gm_params: [[0; 6]; 8],
        };

        // Single-tile scope (parse_tile_info rejects TileCols/TileRows > 1):
        // fixed 2-entry start arrays spanning the whole frame, 1-entry
        // width/height-in-superblocks arrays.
        let mi_col_starts = [0u16, u16::try_from(header.mi_cols).unwrap_or(u16::MAX)];
        let mi_row_starts = [0u16, u16::try_from(header.mi_rows).unwrap_or(u16::MAX)];
        let width_in_sbs_minus_1 = [u16::try_from(header.sb_cols.saturating_sub(1)).unwrap_or(0)];
        let height_in_sbs_minus_1 = [u16::try_from(header.sb_rows.saturating_sub(1)).unwrap_or(0)];

        let mut tile_info_flags = native::StdVideoAV1TileInfoFlags {
            _bitfield_align_1: [],
            _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
        };
        tile_info_flags.set_uniform_tile_spacing_flag(1);
        let tile_info = native::StdVideoAV1TileInfo {
            flags: tile_info_flags,
            TileCols: 1,
            TileRows: 1,
            context_update_tile_id: 0,
            tile_size_bytes_minus_1: 0,
            reserved1: [0; 7],
            // Populated below via `finish()` once the arrays above are
            // pinned in `self` — raw pointers into `self`'s own fields would
            // dangle if built here before `self` is placed on the caller's
            // stack frame.
            pMiColStarts: std::ptr::null(),
            pMiRowStarts: std::ptr::null(),
            pWidthInSbsMinus1: std::ptr::null(),
            pHeightInSbsMinus1: std::ptr::null(),
        };

        Self {
            quantization,
            segmentation,
            loop_filter,
            cdef,
            loop_restoration,
            global_motion,
            tile_info,
            mi_col_starts,
            mi_row_starts,
            width_in_sbs_minus_1,
            height_in_sbs_minus_1,
        }
    }

    /// Wires `self.tile_info`'s pointer fields at the arrays now pinned in
    /// `self` — must be called once, after `self` has its final stack
    /// address (i.e. immediately before use, never before a move).
    pub(crate) const fn finish(&mut self) {
        self.tile_info.pMiColStarts = self.mi_col_starts.as_ptr();
        self.tile_info.pMiRowStarts = self.mi_row_starts.as_ptr();
        self.tile_info.pWidthInSbsMinus1 = self.width_in_sbs_minus_1.as_ptr();
        self.tile_info.pHeightInSbsMinus1 = self.height_in_sbs_minus_1.as_ptr();
    }
}

const fn restoration_type_from_u8(value: u8) -> native::StdVideoAV1FrameRestorationType {
    match value {
        1 => native::STD_VIDEO_AV1_FRAME_RESTORATION_TYPE_WIENER,
        2 => native::STD_VIDEO_AV1_FRAME_RESTORATION_TYPE_SGRPROJ,
        3 => native::STD_VIDEO_AV1_FRAME_RESTORATION_TYPE_SWITCHABLE,
        _ => native::STD_VIDEO_AV1_FRAME_RESTORATION_TYPE_NONE,
    }
}

impl Av1FrameHeader {
    /// Builds the `StdVideoDecodeAV1PictureInfo` this frame header's fields
    /// describe. `optionals` must outlive the returned struct (raw pointer
    /// fields).
    ///
    /// Field names below are `PascalCase`/`camelCase` verbatim from the C
    /// header (see `av1_params.rs`'s module doc) — item-scoped
    /// `#[allow(non_snake_case)]`, not a blanket crate-wide allow.
    #[must_use]
    #[allow(
        non_snake_case,
        reason = "StdVideoDecodeAV1PictureInfo mixes snake_case and \
              PascalCase/camelCase field names verbatim from the C header"
    )]
    pub(crate) fn to_std_picture_info(
        self,
        optionals: &Av1PictureInfoOptionals,
    ) -> native::StdVideoDecodeAV1PictureInfo {
        let mut flags = native::StdVideoDecodeAV1PictureInfoFlags {
            _bitfield_align_1: [],
            _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
        };
        flags.set_error_resilient_mode(1); // KEY_FRAME && show_frame
        flags.set_disable_cdf_update(u32::from(self.disable_cdf_update));
        flags.set_use_superres(0);
        flags.set_render_and_frame_size_different(u32::from(self.render_and_frame_size_different));
        flags.set_allow_screen_content_tools(u32::from(self.allow_screen_content_tools));
        flags.set_is_filter_switchable(0); // FrameIsIntra: not read
        flags.set_force_integer_mv(1); // FrameIsIntra
        flags.set_frame_size_override_flag(u32::from(self.frame_size_override_flag));
        flags.set_buffer_removal_time_present_flag(0);
        flags.set_allow_intrabc(u32::from(self.allow_intrabc));
        flags.set_frame_refs_short_signaling(0); // FrameIsIntra: not read
        flags.set_allow_high_precision_mv(0); // FrameIsIntra: not read
        flags.set_is_motion_mode_switchable(0); // FrameIsIntra: not read
        flags.set_use_ref_frame_mvs(0); // FrameIsIntra: not read
        flags.set_disable_frame_end_update_cdf(u32::from(self.disable_frame_end_update_cdf));
        flags.set_allow_warped_motion(0); // FrameIsIntra: forced 0
        flags.set_reduced_tx_set(u32::from(self.reduced_tx_set));
        flags.set_reference_select(0); // FrameIsIntra: forced 0
        flags.set_skip_mode_present(0); // FrameIsIntra: forced 0
        flags.set_delta_q_present(u32::from(self.delta_q_present));
        flags.set_delta_lf_present(u32::from(self.delta_lf_present));
        flags.set_delta_lf_multi(u32::from(self.delta_lf_multi));
        flags.set_segmentation_enabled(u32::from(self.segmentation_enabled));
        flags.set_segmentation_update_map(u32::from(self.segmentation_update_map));
        flags.set_segmentation_temporal_update(u32::from(self.segmentation_temporal_update));
        flags.set_segmentation_update_data(u32::from(self.segmentation_update_data));
        flags.set_UsesLr(u32::from(self.uses_lr));
        flags.set_usesChromaLr(u32::from(self.uses_chroma_lr));
        flags.set_apply_grain(0); // film grain architecturally excluded
        flags.set_reserved(0);

        let tx_mode = match self.tx_mode {
            0 => native::STD_VIDEO_AV1_TX_MODE_ONLY_4X4,
            2 => native::STD_VIDEO_AV1_TX_MODE_SELECT,
            _ => native::STD_VIDEO_AV1_TX_MODE_LARGEST,
        };

        native::StdVideoDecodeAV1PictureInfo {
            flags,
            frame_type: native::STD_VIDEO_AV1_FRAME_TYPE_KEY,
            current_frame_id: 0,
            OrderHint: self.order_hint,
            primary_ref_frame: native::STD_VIDEO_AV1_PRIMARY_REF_NONE as u8,
            refresh_frame_flags: 0xFF,
            reserved1: 0,
            interpolation_filter: native::STD_VIDEO_AV1_INTERPOLATION_FILTER_EIGHTTAP,
            TxMode: tx_mode,
            delta_q_res: self.delta_q_res,
            delta_lf_res: self.delta_lf_res,
            SkipModeFrame: [0; 2],
            coded_denom: 0,
            reserved2: [0; 3],
            OrderHints: [0; 8],
            expectedFrameId: [0; 8],
            pTileInfo: &raw const optionals.tile_info,
            pQuantization: &raw const optionals.quantization,
            pSegmentation: &raw const optionals.segmentation,
            pLoopFilter: &raw const optionals.loop_filter,
            pCDEF: &raw const optionals.cdef,
            pLoopRestoration: &raw const optionals.loop_restoration,
            pGlobalMotion: &raw const optionals.global_motion,
            pFilmGrain: std::ptr::null(),
        }
    }

    /// Builds this `KEY_FRAME`'s own `StdVideoDecodeAV1ReferenceInfo` for its
    /// setup slot — every key frame refreshes all 8 reference-name slots
    /// (`refresh_frame_flags == 0xFF`), so a future picture reading any of
    /// them needs real data here, mirroring how `decoder_hevc.rs` populates
    /// its own setup slot's reference info even though this round's
    /// `KEY_FRAME`-only scope never itself reads a reference.
    #[must_use]
    #[allow(
        non_snake_case,
        reason = "StdVideoDecodeAV1ReferenceInfo mixes snake_case and \
              PascalCase field names verbatim from the C header"
    )]
    pub(crate) fn to_std_reference_info(self) -> native::StdVideoDecodeAV1ReferenceInfo {
        let mut flags = native::StdVideoDecodeAV1ReferenceInfoFlags {
            _bitfield_align_1: [],
            _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
        };
        flags.set_disable_frame_end_update_cdf(u32::from(self.disable_frame_end_update_cdf));
        flags.set_segmentation_enabled(u32::from(self.segmentation_enabled));
        native::StdVideoDecodeAV1ReferenceInfo {
            flags,
            frame_type: native::STD_VIDEO_AV1_FRAME_TYPE_KEY.0 as u8,
            RefFrameSignBias: 0,
            OrderHint: self.order_hint,
            SavedOrderHints: [0; 8],
        }
    }
}
