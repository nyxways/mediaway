//! Minimal `StdVideoAV1*`/`StdVideoEncodeAV1*` construction for one all-intra
//! Main-profile `KEY_FRAME` — mirrors [`super::h264_params`]/
//! [`super::hevc_params`]'s "smallest self-consistent parameter set" shape,
//! but AV1 has no separate SPS/PPS: one **full** `StdVideoAV1SequenceHeader`
//! (see [`build_sequence_header`]'s doc for why this crate does **not** use
//! `reduced_still_picture_header`, unlike an earlier draft) plus one
//! `StdVideoEncodeAV1PictureInfo` per frame.
//!
//! Deliberately narrow, mirroring `mediaway-encoder-windows`'s D3D12 AV1
//! backend's scope cuts (`d3d12_video_encode/av1.rs`'s
//! `D3D12_VIDEO_ENCODER_AV1_FEATURE_FLAG_NONE`): single 64x64 superblock size
//! (no 128x128), no CDEF/loop restoration/segmentation/film grain/global
//! motion (all point at real, all-disabled structs on
//! [`StdVideoEncodeAV1PictureInfo`] — see [`build_key_frame_picture_info`]'s
//! doc for why these are never left null), fixed constant `base_q_idx`, one
//! operating point, no decoder-model info, `PRIMARY_REF_NONE` (no CDF
//! reference between frames — every pushed frame is an independent key
//! frame, same scope cut as H.264/HEVC's "no GOP/P-frames" this crate
//! already makes).
//!
//! Unlike H.264/HEVC, fetching the header needs no codec-specific `pNext`
//! struct on `VkVideoEncodeSessionParametersGetInfoKHR` — the Vulkan registry
//! defines no `VkVideoEncodeAV1SessionParametersGetInfoKHR` at all, because an
//! AV1 session parameters object stores exactly one sequence header (nothing
//! to select by id, unlike H.264/HEVC's SPS/PPS lists). See
//! [`crate::session_encode::get_encoded_headers_av1`] and the
//! `VK_KHR_video_encode_av1` proposal doc's confirmation that
//! `vkGetEncodedVideoSessionParametersKHR` returns the sequence header as an
//! `OBU_SEQUENCE_HEADER` OBU for this codec too.
//!
//! These are plain-old-data C structs (`vulkanalia::vk::video`), not
//! `vulkanalia::vk::*Khr` structs — see [`super::h264_params`]'s module doc
//! for why constructing/reading them needs no `unsafe`.

#![allow(
    clippy::redundant_pub_crate,
    reason = "workspace `unreachable_pub` policy (Cargo.toml) wants `pub(crate)` here; \
              clippy::pedantic's redundant_pub_crate disagrees for private modules — the \
              two lints are mutually exclusive for this shape, workspace policy wins"
)]
#![allow(
    clippy::cast_possible_truncation,
    reason = "AV1 StdVideo* fields are narrower (u8/u16) than this crate's own u32 pixel \
              dimensions/enum-repr values, but every one here is driver-validated small \
              (this crate's own coded extent, or a fixed spec-level constant like \
              STD_VIDEO_AV1_PRIMARY_REF_NONE == 7) — mirrors session_encode.rs's/ \
              session_command.rs's identical crate-wide allow for small driver-facing counts."
)]

use vulkanalia::vk::video as native;

/// Fixed AV1 `base_q_idx`/`constant_q_index` — all-intra fixed-QP, mirrors
/// `encoder.rs`'s H.264/HEVC `FIXED_QP` but in AV1's wider `0..=255` q-index
/// range (vs. H.264/HEVC's `0..=51`), so it is its own constant rather than
/// reusing `FIXED_QP`.
pub(crate) const FIXED_Q_INDEX: u8 = 128;

/// Number of bits needed to represent `value` in unsigned binary, minimum 1
/// — used for `frame_width_bits_minus_1`/`frame_height_bits_minus_1`, which
/// the AV1 spec defines as "bits needed to hold `max_frame_width_minus_1`" minus 1.
const fn bits_needed(value: u32) -> u32 {
    if value == 0 {
        1
    } else {
        32 - value.leading_zeros()
    }
}

/// `StdVideoAV1ColorConfig` for 4:2:0 8-bit (matches NV12): no monochrome, no
/// separate UV delta-Q, and no explicit colour-description signaling (every
/// `color_*` field left `UNSPECIFIED`/`UNKNOWN` — this crate makes no HDR/wide-gamut
/// claim, matching H.264/HEVC's own lack of VUI colour signaling).
pub(crate) fn build_color_config() -> native::StdVideoAV1ColorConfig {
    let mut flags = native::StdVideoAV1ColorConfigFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
    };
    flags.set_mono_chrome(0);
    flags.set_color_range(0);
    flags.set_separate_uv_delta_q(0);
    flags.set_color_description_present_flag(0);
    native::StdVideoAV1ColorConfig {
        flags,
        BitDepth: 8,
        subsampling_x: 1,
        subsampling_y: 1,
        reserved1: 0,
        color_primaries: native::STD_VIDEO_AV1_COLOR_PRIMARIES_UNSPECIFIED,
        transfer_characteristics: native::STD_VIDEO_AV1_TRANSFER_CHARACTERISTICS_UNSPECIFIED,
        matrix_coefficients: native::STD_VIDEO_AV1_MATRIX_COEFFICIENTS_UNSPECIFIED,
        chroma_sample_position: native::STD_VIDEO_AV1_CHROMA_SAMPLE_POSITION_UNKNOWN,
    }
}

/// All-zero `StdVideoAV1TimingInfo` — provided even though
/// `timing_info_present_flag == 0` (so its content is unused), matching
/// [`build_sequence_header`]'s "never null" fix: an earlier draft left
/// `pTimingInfo` null, which is what `FFmpeg`'s real, hardware-tested
/// `vulkan_encode_av1.c` reference does **not** do (it always provides a
/// real struct there too).
pub(crate) const fn build_timing_info() -> native::StdVideoAV1TimingInfo {
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

/// `StdVideoAV1SequenceHeader` — Main profile, a **full** (not
/// `reduced_still_picture_header`) sequence header with `enable_order_hint`
/// set. An earlier draft used `reduced_still_picture_header` (AV1's
/// narrowest legal sequence variant, forcing order hint/frame-id/timing info
/// all off) reasoning it mirrored [`super::h264_params`]'s/
/// [`super::hevc_params`]'s "smallest self-consistent parameter set" choices
/// — that produced a real-hardware-verified **invalid** bitstream (see
/// `adr/0001`'s AV1 addendum). `FFmpeg`'s own real, hardware-tested
/// `vulkan_encode_av1.c` never uses `reduced_still_picture_header` at all;
/// this crate now mirrors that working reference instead: `enable_order_hint
/// = 1` with a fixed small `order_hint_bits_minus_1`, matching `FFmpeg`'s
/// `av_log2(gop_size)`-derived value for this crate's own "every frame is an
/// independent key frame" case (no real GOP, so any legal value works — `6`
/// mirrors a typical `FFmpeg` default GOP size's derived bit count). Single
/// 64x64 superblock size (`use_128x128_superblock = 0`), no optional coding
/// tool (`enable_*` beyond order hint all `0`).
pub(crate) fn build_sequence_header(
    width: u32,
    height: u32,
    color_config: &native::StdVideoAV1ColorConfig,
    timing_info: &native::StdVideoAV1TimingInfo,
) -> native::StdVideoAV1SequenceHeader {
    let mut flags = native::StdVideoAV1SequenceHeaderFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
    };
    flags.set_still_picture(0);
    flags.set_reduced_still_picture_header(0);
    flags.set_use_128x128_superblock(0);
    flags.set_enable_filter_intra(0);
    flags.set_enable_intra_edge_filter(0);
    flags.set_enable_interintra_compound(0);
    flags.set_enable_masked_compound(0);
    flags.set_enable_warped_motion(0);
    flags.set_enable_dual_filter(0);
    flags.set_enable_order_hint(1);
    flags.set_enable_jnt_comp(0);
    flags.set_enable_ref_frame_mvs(0);
    flags.set_frame_id_numbers_present_flag(0);
    flags.set_enable_superres(0);
    flags.set_enable_cdef(0);
    flags.set_enable_restoration(0);
    flags.set_film_grain_params_present(0);
    flags.set_timing_info_present_flag(0);
    flags.set_initial_display_delay_present_flag(0);

    let max_frame_width_minus_1 = width - 1;
    let max_frame_height_minus_1 = height - 1;
    native::StdVideoAV1SequenceHeader {
        flags,
        seq_profile: native::STD_VIDEO_AV1_PROFILE_MAIN,
        frame_width_bits_minus_1: (bits_needed(max_frame_width_minus_1) - 1) as u8,
        frame_height_bits_minus_1: (bits_needed(max_frame_height_minus_1) - 1) as u8,
        max_frame_width_minus_1: max_frame_width_minus_1 as u16,
        max_frame_height_minus_1: max_frame_height_minus_1 as u16,
        delta_frame_id_length_minus_2: 0,
        additional_frame_id_length_minus_1: 0,
        order_hint_bits_minus_1: 6,
        // `SELECT_SCREEN_CONTENT_TOOLS`/`SELECT_INTEGER_MV` (value `2`) — this
        // crate always "chooses" (opts to signal per-frame rather than fixing
        // a sequence-wide value), matching `FFmpeg`'s reference computation.
        seq_force_screen_content_tools: 2,
        seq_force_integer_mv: 2,
        reserved1: [0; 5],
        pColorConfig: color_config,
        pTimingInfo: timing_info,
    }
}

/// Single operating point, no scalability (`operating_point_idc == 0`), no
/// decoder-model timing, lowest AV1 level (`2.0` — this crate's single small
/// synthetic frame needs nothing higher, mirrors [`super::h264_params`]'s
/// fixed Level 1.0 / [`super::hevc_params`]'s fixed Level 1.0 choices).
pub(crate) fn build_operating_point() -> native::StdVideoEncodeAV1OperatingPointInfo {
    let mut flags = native::StdVideoEncodeAV1OperatingPointInfoFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
    };
    flags.set_decoder_model_present_for_this_op(0);
    flags.set_low_delay_mode_flag(0);
    flags.set_initial_display_delay_present_for_this_op(0);
    native::StdVideoEncodeAV1OperatingPointInfo {
        flags,
        operating_point_idc: 0,
        seq_level_idx: native::STD_VIDEO_AV1_LEVEL_2_0.0 as u8,
        seq_tier: 0,
        decoder_buffer_delay: 0,
        encoder_buffer_delay: 0,
        initial_display_delay_minus_1: 0,
    }
}

/// `StdVideoEncodeAV1ReferenceInfo` describing this `KEY_FRAME` itself, for
/// `VkVideoEncodeAV1DpbSlotInfoKHR::pStdReferenceInfo` on the setup reference
/// slot — every AV1 reference-name mapping this key frame's
/// `refresh_frame_flags = 0xFF` (see [`build_key_frame_picture_info`])
/// refreshes needs a real `RefFrameId`/`frame_type`/`OrderHint` to associate
/// with those slots. `disable_frame_end_update_cdf` mirrors
/// [`build_key_frame_picture_info`]'s own flag of the same name (`0`,
/// matching `FFmpeg`'s real, hardware-tested reference — see that function's
/// doc for why an earlier draft's `1` was wrong). `extension_header` is
/// never null (matching `FFmpeg`'s reference), same reasoning as
/// [`build_key_frame_picture_info`]'s `pExtensionHeader`.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "the returned struct embeds a pointer to `extension_header` itself, so it must \
              borrow caller-owned memory rather than take (and drop) an owned copy"
)]
pub(crate) fn build_reference_info(
    extension_header: &native::StdVideoEncodeAV1ExtensionHeader,
) -> native::StdVideoEncodeAV1ReferenceInfo {
    let mut flags = native::StdVideoEncodeAV1ReferenceInfoFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
    };
    flags.set_disable_frame_end_update_cdf(0);
    flags.set_segmentation_enabled(0);
    native::StdVideoEncodeAV1ReferenceInfo {
        flags,
        RefFrameId: 0,
        frame_type: native::STD_VIDEO_AV1_FRAME_TYPE_KEY,
        OrderHint: 0,
        reserved1: [0; 3],
        pExtensionHeader: extension_header,
    }
}

/// All-disabled `StdVideoAV1LoopFilter` — `loop_filter_delta_enabled == 0`
/// disables per-reference/per-mode deltas entirely, but `loop_filter_ref_deltas`
/// still needs the AV1-spec-default values (`{1,0,0,0,-1,0,-1,-1}`, AV1 spec
/// §7.20's `setup_past_independence`) rather than all-zero: this crate
/// mirrors `FFmpeg`'s real, hardware-tested `vulkan_encode_av1.c` byte-for-byte
/// here since an all-zero array was untested territory this crate could not
/// itself verify.
pub(crate) fn build_loop_filter() -> native::StdVideoAV1LoopFilter {
    let mut flags = native::StdVideoAV1LoopFilterFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
    };
    flags.set_loop_filter_delta_enabled(0);
    flags.set_loop_filter_delta_update(0);
    native::StdVideoAV1LoopFilter {
        flags,
        loop_filter_level: [0; 4],
        loop_filter_sharpness: 0,
        update_ref_delta: 0,
        loop_filter_ref_deltas: [1, 0, 0, 0, -1, 0, -1, -1],
        update_mode_delta: 1,
        loop_filter_mode_deltas: [0; 2],
    }
}

/// All-disabled `StdVideoAV1CDEF` (`cdef_bits == 0` — no CDEF strength
/// selection signaled, matches `enable_cdef == 0` on the sequence header).
pub(crate) const fn build_cdef() -> native::StdVideoAV1CDEF {
    native::StdVideoAV1CDEF {
        cdef_damping_minus_3: 0,
        cdef_bits: 0,
        cdef_y_pri_strength: [0; 8],
        cdef_y_sec_strength: [0; 8],
        cdef_uv_pri_strength: [0; 8],
        cdef_uv_sec_strength: [0; 8],
    }
}

/// All-disabled `StdVideoAV1LoopRestoration` (`FrameRestorationType == NONE`
/// for every plane, matches `enable_restoration == 0` on the sequence
/// header).
pub(crate) const fn build_loop_restoration() -> native::StdVideoAV1LoopRestoration {
    native::StdVideoAV1LoopRestoration {
        FrameRestorationType: [
            native::STD_VIDEO_AV1_FRAME_RESTORATION_TYPE_NONE,
            native::STD_VIDEO_AV1_FRAME_RESTORATION_TYPE_NONE,
            native::STD_VIDEO_AV1_FRAME_RESTORATION_TYPE_NONE,
        ],
        LoopRestorationSize: [1, 1, 1],
    }
}

/// All-identity `StdVideoAV1GlobalMotion` (`GmType == IDENTITY (0)` for every
/// reference — no global motion this stage, matches `PRIMARY_REF_NONE`/no
/// inter-frame prediction).
pub(crate) const fn build_global_motion() -> native::StdVideoAV1GlobalMotion {
    native::StdVideoAV1GlobalMotion {
        GmType: [0; 8],
        gm_params: [[0; 6]; 8],
    }
}

/// All-disabled `StdVideoAV1Segmentation` (`FeatureEnabled == 0` for every
/// segment, matches `segmentation_enabled == 0` on the picture-info flags).
pub(crate) const fn build_segmentation() -> native::StdVideoAV1Segmentation {
    native::StdVideoAV1Segmentation {
        FeatureEnabled: [0; 8],
        FeatureData: [[0; 8]; 8],
    }
}

/// `StdVideoEncodeAV1ExtensionHeader` — always provided (even though
/// `generate_obu_extension_header == false` means the driver does not use
/// it), matching `FFmpeg`'s `vulkan_encode_av1.c` reference: `pExtensionHeader`
/// is never null there either.
pub(crate) const fn build_extension_header() -> native::StdVideoEncodeAV1ExtensionHeader {
    native::StdVideoEncodeAV1ExtensionHeader {
        temporal_id: 0,
        spatial_id: 0,
    }
}

/// Fixed-QP quantization: `base_q_idx` set to [`FIXED_Q_INDEX`], no
/// quantization matrix (`using_qmatrix == 0`), no separate per-plane
/// delta-Q, matching [`super::h264_params::build_pps`]'s single fixed
/// `constant_qp` reasoning.
pub(crate) fn build_quantization() -> native::StdVideoAV1Quantization {
    let mut flags = native::StdVideoAV1QuantizationFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
    };
    flags.set_using_qmatrix(0);
    flags.set_diff_uv_delta(0);
    native::StdVideoAV1Quantization {
        flags,
        base_q_idx: FIXED_Q_INDEX,
        DeltaQYDc: 0,
        DeltaQUDc: 0,
        DeltaQUAc: 0,
        DeltaQVDc: 0,
        DeltaQVAc: 0,
        qm_y: 0,
        qm_u: 0,
        qm_v: 0,
    }
}

/// Every optional `StdVideoEncodeAV1PictureInfo` pointer field's pointee,
/// bundled so [`build_key_frame_picture_info`] doesn't trip
/// `clippy::too_many_arguments` — the caller keeps this alive on its own
/// stack frame for the duration of one `vkCmdEncodeVideoKHR` call (same
/// pattern as [`super::h264_params::build_idr_picture_info`]'s `pRefLists`).
pub(crate) struct PictureInfoOptionals {
    pub(crate) quantization: native::StdVideoAV1Quantization,
    pub(crate) segmentation: native::StdVideoAV1Segmentation,
    pub(crate) loop_filter: native::StdVideoAV1LoopFilter,
    pub(crate) cdef: native::StdVideoAV1CDEF,
    pub(crate) loop_restoration: native::StdVideoAV1LoopRestoration,
    pub(crate) global_motion: native::StdVideoAV1GlobalMotion,
    pub(crate) extension_header: native::StdVideoEncodeAV1ExtensionHeader,
}

impl PictureInfoOptionals {
    pub(crate) fn new() -> Self {
        Self {
            quantization: build_quantization(),
            segmentation: build_segmentation(),
            loop_filter: build_loop_filter(),
            cdef: build_cdef(),
            loop_restoration: build_loop_restoration(),
            global_motion: build_global_motion(),
            extension_header: build_extension_header(),
        }
    }
}

/// `StdVideoEncodeAV1PictureInfo` for one independent `KEY_FRAME` —
/// `PRIMARY_REF_NONE` (no CDF carried over from a previous frame),
/// `refresh_frame_flags = 0xFF` (a key frame refreshes every one of AV1's 8
/// reference-frame slots per spec). `disable_cdf_update`/
/// `disable_frame_end_update_cdf` are **not** set here (`0`, i.e. CDF
/// adaptation runs normally within the frame) — an earlier draft set both to
/// `1` reasoning "no forward-adapted CDF state needs preserving," which
/// produced a real-hardware-verified **invalid** bitstream (see
/// `adr/0001`'s AV1 addendum); `FFmpeg`'s own real, hardware-tested
/// `vulkan_encode_av1.c` leaves both `0` even for key frames, and this crate
/// now mirrors that working reference. `pTileInfo` is null (no single-tile
/// `StdVideoAV1TileInfo` struct built or chained at all) — `FFmpeg`'s own real,
/// hardware-tested `vulkan_encode_av1.c` leaves this field null too (with an
/// open, unresolved comment in their source), and this crate mirrors that working
/// reference rather than a struct this crate could not itself verify against
/// real hardware. `pSegmentation`/`pLoopFilter`/`pCDEF`/`pLoopRestoration`/
/// `pGlobalMotion`/`pExtensionHeader` are **never** null (unlike an earlier
/// draft) — they point at `optionals`' all-disabled structs; a null pointer
/// there was the other real-hardware-verified bug this addendum fixed,
/// matching `FFmpeg`'s reference which also never passes null for these six
/// fields.
pub(crate) fn build_key_frame_picture_info(
    width: u32,
    height: u32,
    optionals: &PictureInfoOptionals,
) -> native::StdVideoEncodeAV1PictureInfo {
    let mut flags = native::StdVideoEncodeAV1PictureInfoFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 4]),
    };
    // `FFmpeg`'s reference sets this for every I/IDR frame whose display order
    // hasn't passed its encode order — true for this crate's own case (every
    // pushed frame is an independent key frame, encoded in display order).
    flags.set_error_resilient_mode(1);
    flags.set_disable_cdf_update(0);
    flags.set_use_superres(0);
    flags.set_render_and_frame_size_different(0);
    flags.set_allow_screen_content_tools(0);
    flags.set_is_filter_switchable(0);
    flags.set_force_integer_mv(0);
    flags.set_frame_size_override_flag(0);
    flags.set_buffer_removal_time_present_flag(0);
    flags.set_allow_intrabc(0);
    flags.set_frame_refs_short_signaling(0);
    flags.set_allow_high_precision_mv(0);
    flags.set_is_motion_mode_switchable(0);
    flags.set_use_ref_frame_mvs(0);
    flags.set_disable_frame_end_update_cdf(0);
    flags.set_allow_warped_motion(0);
    flags.set_reduced_tx_set(0);
    flags.set_skip_mode_present(0);
    flags.set_delta_q_present(0);
    flags.set_delta_lf_present(0);
    flags.set_delta_lf_multi(0);
    flags.set_segmentation_enabled(0);
    flags.set_segmentation_update_map(0);
    flags.set_segmentation_temporal_update(0);
    flags.set_segmentation_update_data(0);
    flags.set_UsesLr(0);
    flags.set_usesChromaLr(0);
    flags.set_show_frame(1);
    flags.set_showable_frame(0);

    native::StdVideoEncodeAV1PictureInfo {
        flags,
        frame_type: native::STD_VIDEO_AV1_FRAME_TYPE_KEY,
        frame_presentation_time: 0,
        current_frame_id: 0,
        order_hint: 0,
        primary_ref_frame: native::STD_VIDEO_AV1_PRIMARY_REF_NONE as u8,
        refresh_frame_flags: 0xFF,
        coded_denom: 0,
        render_width_minus_1: (width - 1) as u16,
        render_height_minus_1: (height - 1) as u16,
        interpolation_filter: native::STD_VIDEO_AV1_INTERPOLATION_FILTER_EIGHTTAP,
        TxMode: native::STD_VIDEO_AV1_TX_MODE_SELECT,
        delta_q_res: 0,
        delta_lf_res: 0,
        ref_order_hint: [0; 8],
        ref_frame_idx: [-1; 7],
        reserved1: [0; 3],
        delta_frame_id_minus_1: [0; 7],
        pTileInfo: std::ptr::null(),
        pQuantization: &raw const optionals.quantization,
        pSegmentation: &raw const optionals.segmentation,
        pLoopFilter: &raw const optionals.loop_filter,
        pCDEF: &raw const optionals.cdef,
        pLoopRestoration: &raw const optionals.loop_restoration,
        pGlobalMotion: &raw const optionals.global_motion,
        pExtensionHeader: &raw const optionals.extension_header,
        pBufferRemovalTimes: std::ptr::null(),
    }
}
