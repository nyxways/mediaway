//! VA-API HEVC CPU-upload encode session — Main profile, single-forward-reference P-frame GOP.
//!
//! See [ADR-0003](../../adr/linux/0003-vaapi-hevc-p-frame-gop.md): binding choice, scope (Main
//! profile / CQP / CPU upload only, `VAEntrypointEncSlice`), the fresh VA-API parameter-buffer
//! design (VA-API's `EncSequenceParameterBufferHEVC` has no `StdVideoH265*`-equivalent field set
//! — the driver synthesizes the real VPS/SPS/PPS NAL units itself), and the zero-hardware-
//! verification caveat. [`hevc_gop`](super::hevc_gop) is the ported `GopState` decision state
//! machine driving `PicOrderCntVal`/reference-list construction below — see that module's doc.
//!
//! Structurally mirrors [`super::video::VaapiVideoEncoder`]'s session shape and `push_frame`
//! sequencing; [`super::video::probe_supports_p_frames`]/[`super::video::upload_cpu_nv12`]/
//! [`super::video::nv12_size`] are reused directly (`pub(super)`) rather than duplicated — all
//! three are genuinely codec-agnostic, mirroring [`super::gop::WORKSPACE_DPB_CAP`]'s own
//! same-crate reuse precedent (ADR-0003 § Context explicitly rejects importing
//! `crate::vulkan::hevc_gop` directly for the cross-*crate* case; same-crate, same-`vaapi`-tree
//! reuse of genuinely shared helpers is a different, already-established axis).
//!
//! Deliberately disables SAO (`sample_adaptive_offset_enabled_flag`) and temporal MVP
//! (`sps_temporal_mvp_enabled_flag`) in the encoded SPS — a real compression-efficiency
//! trade-off, made to keep this encoder's output the simplest possible shape for this
//! workspace's own sibling VA-API HEVC *decoder* (`mediaway-decoder` ADR-0003) to round-trip
//! correctly (see this ADR's § Alternatives Considered).

use std::collections::VecDeque;
use std::rc::Rc;

use crate::{EncodeError, VideoEncoder, VideoEncoderConfig, VideoInputPreference};
use mediaway_common::{
    Bytes, CodecKind, Packet, PixelFormat, StreamInfo, VideoFrame, VideoFrameStorage, VideoGeometry,
};

use cros_libva::{
    BufferType, Config, Context, Display, EncPictureParameter, EncSequenceParameter,
    EncSliceParameter, HEVCEncPicFields, HEVCEncSeqFields, HevcEncPicSccFields,
    HevcEncSeqSccFields, HevcEncSliceFields, MappedCodedBuffer, Picture, PictureHEVC, Surface,
    UsageHint, VA_FOURCC_NV12, VA_INVALID_SURFACE, VA_PICTURE_HEVC_INVALID,
    VA_PICTURE_HEVC_RPS_ST_CURR_BEFORE, VA_RC_CQP, VA_RT_FORMAT_YUV420, VAConfigAttrib,
    VAConfigAttribType, VAEntrypoint,
};

use super::codec::video_profile;
use super::hevc_gop::{DpbSlot, FrameDecision, FrameRequest, GopState};

/// Number of driver surfaces kept in rotation — aliases [`super::gop::WORKSPACE_DPB_CAP`], same
/// reasoning as [`super::video::SURFACE_POOL_SIZE`] (redeclared here rather than importing that
/// crate-private `const`, since a bare numeric alias carries no real duplication risk — the
/// single source of truth is `gop::WORKSPACE_DPB_CAP` itself, which both aliases read from).
const SURFACE_POOL_SIZE: usize = super::gop::WORKSPACE_DPB_CAP;

/// Fixed quantization parameter used under `VA_RC_CQP` (range 0..=51) — same value and rationale
/// as [`super::video::FIXED_QP`] (this stage exposes no quality/bitrate knob, ADR-0001 § Scope).
const FIXED_QP: u8 = 26;

/// HEVC Main profile `general_profile_idc` (ITU-T H.265 Table A.1).
const GENERAL_PROFILE_IDC_MAIN: u8 = 1;
/// Main tier (not High tier).
const GENERAL_TIER_FLAG_MAIN: u8 = 0;
/// Level 3.1 (`general_level_idc == 30 * level_number`, i.e. `93`) — generous for small CQP test
/// frames while allowing reasonably-sized real content, mirroring
/// [`super::video::LEVEL_IDC`]'s identical "generous, fixed, never queried" disposition for
/// H.264 Level 3.0.
const GENERAL_LEVEL_IDC: u8 = 93;

// CU/TU log2 sizes — the numeric CU/TU coding-block/transform-block size-range choice this ADR
// cites (not ports) from `mediaway-encoder::vulkan::hevc_params` (CTU `8x8..32x32`, TU
// `4x4..32x32`, transform-hierarchy depth `3`), since that numeric choice transfers directly
// regardless of which codec API constructs the parameter set (ADR-0003 § References).
const CB_MIN_LOG2_MINUS3: u8 = 0; // log2(8) - 3
const CB_DIFF_LOG2: u8 = 2; // log2(32) - log2(8)
const TB_MIN_LOG2_MINUS2: u8 = 0; // log2(4) - 2
const TB_DIFF_LOG2: u8 = 3; // log2(32) - log2(4)
const TRANSFORM_HIERARCHY_DEPTH: u8 = 3; // == TB_DIFF_LOG2, the legal maximum for this range
/// Coding tree unit size in luma samples (`8 << CB_DIFF_LOG2`), derived from the CU range above.
const CTU_SIZE: u32 = 32;

/// `ctu_max_bitsize_allowed`'s "no limit" sentinel value — inferred from general VA-API
/// sample-code convention, **not independently confirmed** against a real driver or `FFmpeg`
/// source this session (ADR-0003 § Open questions #2, partially closed by this ADR's own
/// Addendum: the field itself is confirmed present as a real `u8`, but not this numeric
/// convention).
const CTU_MAX_BITSIZE_NO_LIMIT: u8 = 0xFF;

/// VA-API HEVC encode session (Main profile, CQP, CPU NV12 upload). See module docs for scope.
///
/// `gop_size <= 1` (the default) or a driver that does not report
/// `VAConfigAttribEncMaxRefFrames` support both fall back to all-IDR encode — same fallback
/// contract as [`super::video::VaapiVideoEncoder`] (ADR-0002/ADR-0003 share this disposition).
pub(crate) struct VaapiHevcVideoEncoder {
    context: Rc<Context>,
    /// Kept alive for the context's lifetime — mirrors
    /// [`super::video::VaapiVideoEncoder::_config`]'s identical rationale.
    _config: Config,
    info: StreamInfo,
    width: u32,
    height: u32,
    nv12_bytes: usize,
    bits_per_second: u32,
    surfaces: Vec<Option<Surface<()>>>,
    /// GOP decision state (ADR-0003) — see [`super::hevc_gop`].
    gop: GopState,
    /// The GOP size this session actually honors — `1` when GOP mode is disabled, `config.gop_size`
    /// otherwise. Needed so [`build_seq_params`] can recompute `intra_period`/`intra_idr_period`/
    /// `ip_period` fresh every time the SPS is actually sent (once per IDR); unlike H.264
    /// (`VaapiVideoEncoder::effective_gop_size`), this value does **not** also gate any per-frame
    /// picture-parameter decision — `reference_pic_flag` is unconditionally `1` here (every
    /// picture in this single-forward-reference design is a candidate reference), so there is no
    /// H.264-style "GOP mode active vs disabled" ambiguity a bare `FrameDecision` would need to
    /// resolve for anything but the SPS itself (see ADR-0003 § "`hevc.rs::VaapiHevcVideoEncoder`
    /// mirrors `video.rs`'s field shape").
    effective_gop_size: u32,
    pending: VecDeque<Packet>,
    flushed: bool,
}

impl VaapiHevcVideoEncoder {
    /// Open according to [`VideoEncoderConfig::input`].
    pub(crate) fn open(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        validate(config)?;
        match config.input {
            VideoInputPreference::CpuUploadOk => Self::open_cpu(config),
            _ => Err(EncodeError::Unsupported),
        }
    }

    fn open_cpu(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        // See `super::video::VaapiVideoEncoder::open_cpu`'s identical doc comment for why
        // `Display::open()` honestly returns `None` in this session's environment.
        let display: Rc<Display> = Display::open().ok_or(EncodeError::Backend)?;

        let profile = video_profile(config.codec)?;

        // ADR-0003 capability gate, reusing `super::video::probe_supports_p_frames` directly —
        // that function is already parameterized by `profile`, so no HEVC-specific copy is
        // needed (see module doc).
        let supports_p_frames = super::video::probe_supports_p_frames(&display, profile);
        let effective_gop_size = if config.gop_size > 1 && supports_p_frames {
            config.gop_size
        } else {
            1
        };

        let attrs = vec![VAConfigAttrib {
            type_: VAConfigAttribType::VAConfigAttribRateControl,
            value: VA_RC_CQP,
        }];
        let vaconfig = display
            .create_config(attrs, profile, VAEntrypoint::VAEntrypointEncSlice)
            .map_err(|_| EncodeError::Backend)?;

        let surfaces = display
            .create_surfaces(
                VA_RT_FORMAT_YUV420,
                Some(VA_FOURCC_NV12),
                config.width,
                config.height,
                Some(UsageHint::USAGE_HINT_ENCODER),
                vec![(); SURFACE_POOL_SIZE],
            )
            .map_err(|_| EncodeError::Backend)?;

        let context = display
            .create_context(
                &vaconfig,
                config.width,
                config.height,
                Some(&surfaces),
                true,
            )
            .map_err(|_| EncodeError::Backend)?;

        let nv12_bytes = super::video::nv12_size(config.width, config.height)?;

        Ok(Self {
            context,
            _config: vaconfig,
            info: stream_info_from(config),
            width: config.width,
            height: config.height,
            nv12_bytes,
            bits_per_second: config.bitrate_bps,
            surfaces: surfaces.into_iter().map(Some).collect(),
            gop: GopState::new(effective_gop_size),
            effective_gop_size,
            pending: VecDeque::new(),
            flushed: false,
        })
    }

    /// Encode one frame from `surface` (already holding the uploaded NV12 picture) per
    /// `decision` (ADR-0003's `GopState::decide` output), reading `reference`'s
    /// `(VASurfaceID, DpbSlot)` as `ReferenceFrames[0]`/`RefPicList0[0]` when this is a P frame.
    /// Mirrors [`super::video::VaapiVideoEncoder::encode_one`]'s shape and lost-surface
    /// disposition (see that method's doc comment for the full rationale).
    fn encode_one(
        &self,
        surface: Surface<()>,
        frame: &VideoFrame,
        decision: &FrameDecision,
        reference: Option<(cros_libva::VASurfaceID, DpbSlot)>,
    ) -> (Option<Surface<()>>, Result<Packet, EncodeError>) {
        let Ok(pic_width) = u16::try_from(self.width) else {
            return (Some(surface), Err(EncodeError::InvalidInput));
        };
        let Ok(pic_height) = u16::try_from(self.height) else {
            return (Some(surface), Err(EncodeError::InvalidInput));
        };
        let num_ctu_in_slice = ctu_count(self.width, self.height);

        // Generous size hint — see `VaapiVideoEncoder::encode_one`'s identical comment.
        let coded_size = self.nv12_bytes.saturating_mul(2).max(4096);

        let Ok(coded_buf) = self.context.create_enc_coded(coded_size) else {
            return (Some(surface), Err(EncodeError::Backend));
        };

        let surface_id = surface.id();

        // EncSequenceParameter is sent only on IDR frames once GOP mode is active — same
        // bandwidth-efficiency motivation as ADR-0002's identical gate for H.264.
        let seq_buf = if decision.is_idr {
            let seq_params = build_seq_params(
                pic_width,
                pic_height,
                self.bits_per_second,
                self.effective_gop_size,
            );
            match self.context.create_buffer(BufferType::EncSequenceParameter(
                EncSequenceParameter::HEVC(seq_params),
            )) {
                Ok(buf) => Some(buf),
                Err(_) => return (Some(surface), Err(EncodeError::Backend)),
            }
        } else {
            None
        };

        let pic_params = build_pic_params(surface_id, coded_buf.id(), decision, reference);
        let slice_params = build_slice_params(num_ctu_in_slice, decision, reference);

        let Ok(pic_buf) =
            self.context
                .create_buffer(BufferType::EncPictureParameter(EncPictureParameter::HEVC(
                    pic_params,
                )))
        else {
            return (Some(surface), Err(EncodeError::Backend));
        };
        let Ok(slice_buf) =
            self.context
                .create_buffer(BufferType::EncSliceParameter(EncSliceParameter::HEVC(
                    slice_params,
                )))
        else {
            return (Some(surface), Err(EncodeError::Backend));
        };

        let timestamp = u64::try_from(frame.pts).unwrap_or(0);
        let mut picture = Picture::new::<()>(timestamp, Rc::clone(&self.context), surface);
        if let Some(seq_buf) = seq_buf {
            picture.add_buffer(seq_buf);
        }
        picture.add_buffer(pic_buf);
        picture.add_buffer(slice_buf);

        let Ok(picture) = picture.begin::<()>() else {
            return (None, Err(EncodeError::Backend));
        };
        let Ok(picture) = picture.render() else {
            return (None, Err(EncodeError::Backend));
        };
        let Ok(picture) = picture.end() else {
            return (None, Err(EncodeError::Backend));
        };
        let Ok(picture) = picture.sync::<()>() else {
            return (None, Err(EncodeError::Backend));
        };

        let bytes = match MappedCodedBuffer::new(&coded_buf) {
            Ok(mapped) => {
                let mut bytes = Vec::new();
                for segment in mapped.iter() {
                    bytes.extend_from_slice(segment.buf);
                }
                bytes
            }
            Err(_) => return (picture.take_surface().ok(), Err(EncodeError::Backend)),
        };

        let surface = picture.take_surface().ok();
        let packet = Packet {
            stream_id: 0,
            pts: frame.pts,
            dts: frame.pts,
            duration: frame.duration,
            is_keyframe: decision.is_idr,
            is_discard: false,
            payload: Bytes::from(bytes),
        };
        (surface, Ok(packet))
    }
}

impl VideoEncoder for VaapiHevcVideoEncoder {
    fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    fn push_frame(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        if self.flushed {
            return Err(EncodeError::Closed);
        }
        let VideoFrameStorage::Cpu { data } = &frame.storage else {
            return Err(EncodeError::Unsupported);
        };
        if frame.width != self.width || frame.height != self.height {
            return Err(EncodeError::InvalidInput);
        }
        if data.len() < self.nv12_bytes {
            return Err(EncodeError::InvalidInput);
        }

        let decision = self.gop.decide(FrameRequest::Auto);

        // Lost-reference-surface guard — mirrors `VaapiVideoEncoder::push_frame`'s identical
        // step 3 (ADR-0002 § Real gap found; applies unchanged to this ADR's own single-slot
        // reference tracking).
        let reference = match decision.reference {
            Some((ref_slot, ref_dpb_slot)) => {
                let ref_surface = self.surfaces[ref_slot]
                    .as_ref()
                    .ok_or(EncodeError::Backend)?;
                Some((ref_surface.id(), ref_dpb_slot))
            }
            None => None,
        };

        let slot = decision.setup_slot;
        let surface = self.surfaces[slot].take().ok_or(EncodeError::Backend)?;

        let surface = match super::video::upload_cpu_nv12(&surface, data, self.width, self.height) {
            Ok(()) => surface,
            Err(e) => {
                self.surfaces[slot] = Some(surface);
                return Err(e);
            }
        };

        let (returned_surface, result) = self.encode_one(surface, frame, &decision, reference);
        // See `VaapiVideoEncoder::push_frame`'s identical step-7 comment for why `returned_surface`
        // may be `None` on a failed `Picture` step, and why that is handled honestly here rather
        // than fabricated.
        self.surfaces[slot] = returned_surface;
        let packet = result?;
        self.pending.push_back(packet);
        Ok(())
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        Ok(self.pending.pop_front())
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        // Every `push_frame` already runs its encode synchronously — see
        // `VaapiVideoEncoder::flush`'s identical rationale.
        self.flushed = true;
        Ok(())
    }
}

/// Sequence parameter set — sent only on IDR frames once GOP mode is active (see `encode_one`).
/// Deliberately disables SAO/temporal-MVP (see module doc) and enables AMP/strong-intra-smoothing
/// (real, spec-legal quality features with no scope cost). `effective_gop_size <= 1` reproduces
/// this crate's pre-ADR-0003 all-IDR cadence (`intra_period`/`intra_idr_period`/`ip_period` =
/// `1`/`1`/`0`, mirroring [`super::video::build_seq_params`]'s identical H.264 disposition).
fn build_seq_params(
    pic_width: u16,
    pic_height: u16,
    bits_per_second: u32,
    effective_gop_size: u32,
) -> cros_libva::EncSequenceParameterBufferHEVC {
    let gop_active = effective_gop_size > 1;
    let (intra_period, intra_idr_period, ip_period) = if gop_active {
        (effective_gop_size, 1, 1)
    } else {
        (1, 1, 0)
    };

    let seq_fields = HEVCEncSeqFields::new(
        1, // chroma_format_idc: 4:2:0
        0, // separate_colour_plane_flag
        0, // bit_depth_luma_minus8: 8-bit
        0, // bit_depth_chroma_minus8: 8-bit
        0, // scaling_list_enabled_flag
        1, // strong_intra_smoothing_enabled_flag: real quality feature, no scope cost
        1, // amp_enabled_flag: real quality feature, no scope cost
        // sample_adaptive_offset_enabled_flag: disabled — keeps this encoder's slice-header
        // shape the simplest possible for the sibling VA-API HEVC decoder to round-trip (module
        // doc / ADR-0003 § Alternatives Considered).
        0, 0, // pcm_enabled_flag
        0, // pcm_loop_filter_disabled_flag: unreachable, PCM disabled
        // sps_temporal_mvp_enabled_flag: disabled, same decoder-simplicity reasoning as SAO.
        0,
        1, // low_delay_seq: matches this design's "no reordering, no B-frames" shape exactly
        0, // hierachical_flag [sic, real cros-libva field name]: no hierarchical GOP
    );

    cros_libva::EncSequenceParameterBufferHEVC::new(
        GENERAL_PROFILE_IDC_MAIN,
        GENERAL_LEVEL_IDC,
        GENERAL_TIER_FLAG_MAIN,
        intra_period,
        intra_idr_period,
        ip_period,
        bits_per_second,
        pic_width,
        pic_height,
        &seq_fields,
        CB_MIN_LOG2_MINUS3,
        CB_DIFF_LOG2,
        TB_MIN_LOG2_MINUS2,
        TB_DIFF_LOG2,
        TRANSFORM_HIERARCHY_DEPTH,    // max_transform_hierarchy_depth_inter
        TRANSFORM_HIERARCHY_DEPTH,    // max_transform_hierarchy_depth_intra
        0,                            // pcm_sample_bit_depth_luma_minus1: unused, PCM disabled
        0,                            // pcm_sample_bit_depth_chroma_minus1: unused
        0,                            // log2_min_pcm_luma_coding_block_size_minus3: unused
        0,                            // log2_max_pcm_luma_coding_block_size_minus3: unused
        None,                         // vui_fields: no VUI this stage
        0,                            // aspect_ratio_idc
        0,                            // sar_width
        0,                            // sar_height
        0,                            // vui_num_units_in_tick
        0,                            // vui_time_scale
        0,                            // min_spatial_segmentation_idc
        0,                            // max_bytes_per_pic_denom
        0,                            // max_bits_per_min_cu_denom
        &HevcEncSeqSccFields::new(0), // palette_mode_enabled_flag: no screen-content-coding
    )
}

/// Picture parameter set. `decision.is_idr` drives `idr_pic_flag`/`coding_type`/`nal_unit_type`;
/// `reference_pic_flag` is **unconditionally** `1` (unlike H.264's `gop_active`-gated flag — see
/// [`VaapiHevcVideoEncoder::effective_gop_size`]'s own doc comment for why this asymmetry is
/// safe here): every picture in this single-forward-reference design is a candidate future
/// reference. `reference`, when `Some`, becomes `ReferenceFrames[0]`; the other 14 entries stay
/// [`invalid_picture_hevc`].
fn build_pic_params(
    surface_id: cros_libva::VASurfaceID,
    coded_buf_id: cros_libva::VABufferID,
    decision: &FrameDecision,
    reference: Option<(cros_libva::VASurfaceID, DpbSlot)>,
) -> cros_libva::EncPictureParameterBufferHEVC {
    let decoded_curr_pic = PictureHEVC::new(surface_id, decision.poc, 0);
    let mut reference_frames: [PictureHEVC; 15] = std::array::from_fn(|_| invalid_picture_hevc());
    if let Some((ref_surface_id, ref_slot)) = reference {
        reference_frames[0] = reference_picture_hevc(ref_surface_id, ref_slot);
    }

    // FFmpeg's real `vaapi_encode_h265_init_picture_params` numeric convention: `1` = Intra,
    // `2` = Predictive, `3` = Bipredictive (never used here — no B-frames).
    let coding_type: u32 = if decision.is_idr { 1 } else { 2 };
    let pic_fields = HEVCEncPicFields::new(
        u32::from(decision.is_idr), // idr_pic_flag
        coding_type,
        1, // reference_pic_flag: see doc comment above
        0, // dependent_slice_segments_enabled_flag
        0, // sign_data_hiding_enabled_flag
        1, // constrained_intra_pred_flag
        0, // transform_skip_enabled_flag
        0, // cu_qp_delta_enabled_flag
        0, // weighted_pred_flag
        0, // weighted_bipred_flag
        0, // transquant_bypass_enabled_flag
        0, // tiles_enabled_flag
        0, // entropy_coding_sync_enabled_flag
        0, // loop_filter_across_tiles_enabled_flag: unreachable, no tiles
        0, // pps_loop_filter_across_slices_enabled_flag
        0, // scaling_list_data_present_flag
        0, // screen_content_flag
        0, // enable_gpu_weighted_prediction
        0, // no_output_of_prior_pics_flag
    );

    // FFmpeg: `HEVC_NAL_IDR_W_RADL` (`19`) for IDR, `HEVC_NAL_TRAIL_R` (`1`, a
    // reference-picture trailing picture — not `HEVC_NAL_TRAIL_N`/`0`, since every P picture
    // here is a reference) for P.
    let nal_unit_type: u8 = if decision.is_idr { 19 } else { 1 };

    cros_libva::EncPictureParameterBufferHEVC::new(
        decoded_curr_pic,
        reference_frames,
        coded_buf_id,
        0, // collocated_ref_pic_index: unused, no temporal MVP
        0, // last_picture: no end-of-sequence/stream signaling this stage
        FIXED_QP,
        0,         // diff_cu_qp_delta_depth
        0,         // pps_cb_qp_offset
        0,         // pps_cr_qp_offset
        0,         // num_tile_columns_minus1
        0,         // num_tile_rows_minus1
        [0u8; 19], // column_width_minus1: unused, no tiles
        [0u8; 21], // row_height_minus1: unused, no tiles
        0,         // log2_parallel_merge_level_minus2
        CTU_MAX_BITSIZE_NO_LIMIT,
        0, // num_ref_idx_l0_default_active_minus1: exactly one active L0 reference when present
        0, // num_ref_idx_l1_default_active_minus1: unused, no B slices
        0, // slice_pic_parameter_set_id
        nal_unit_type,
        &pic_fields,
        0,                            // hierarchical_level_plus1: no hierarchical GOP
        0,                            // va_byte_reserved
        &HevcEncPicSccFields::new(0), // pps_curr_pic_ref_enabled_flag: no screen-content-coding
    )
}

/// Single-slice-per-picture slice parameters covering the whole picture. `decision.is_idr`
/// selects `slice_type` (I vs P, matching this workspace's own decode-side `HevcSliceType`
/// numeric convention: `0 = B, 1 = P, 2 = I`); `reference`, when `Some`, becomes
/// `ref_pic_list0[0]` — the sole entry this crate's single-forward-reference design ever
/// populates.
fn build_slice_params(
    num_ctu_in_slice: u32,
    decision: &FrameDecision,
    reference: Option<(cros_libva::VASurfaceID, DpbSlot)>,
) -> cros_libva::EncSliceParameterBufferHEVC {
    let mut ref_pic_list0: [PictureHEVC; 15] = std::array::from_fn(|_| invalid_picture_hevc());
    let ref_pic_list1: [PictureHEVC; 15] = std::array::from_fn(|_| invalid_picture_hevc());
    if let Some((ref_surface_id, ref_slot)) = reference {
        ref_pic_list0[0] = reference_picture_hevc(ref_surface_id, ref_slot);
    }
    let slice_type: u8 = if decision.is_idr { 2 } else { 1 };

    let slice_fields = HevcEncSliceFields::new(
        1, // last_slice_of_pic_flag: single slice per picture
        0, // dependent_slice_segment_flag
        0, // colour_plane_id: unused, chroma_format_idc != 3
        0, // slice_temporal_mvp_enabled_flag: disabled at SPS level
        0, // slice_sao_luma_flag: SAO disabled at SPS level
        0, // slice_sao_chroma_flag
        0, // num_ref_idx_active_override_flag: use the picture-level default (1)
        0, // mvd_l1_zero_flag: unused, no B slices
        0, // cabac_init_flag: unused, cabac_init_present_flag not set
        0, // slice_deblocking_filter_disabled_flag: filter enabled
        0, // slice_loop_filter_across_slices_enabled_flag
        1, // collocated_from_l0_flag: default L0 (inert, temporal MVP disabled)
    );

    cros_libva::EncSliceParameterBufferHEVC::new(
        0, // slice_segment_address: whole picture in one slice
        num_ctu_in_slice,
        slice_type,
        0, // slice_pic_parameter_set_id
        0, // num_ref_idx_l0_active_minus1: exactly one active L0 reference when present
        0, // num_ref_idx_l1_active_minus1: unused, no B slices
        ref_pic_list0,
        ref_pic_list1,
        0, // luma_log2_weight_denom: unused, no weighted prediction
        0, // delta_chroma_log2_weight_denom
        [0i8; 15],
        [0i8; 15],
        [[0i8; 2]; 15],
        [[0i8; 2]; 15],
        [0i8; 15],
        [0i8; 15],
        [[0i8; 2]; 15],
        [[0i8; 2]; 15],
        5, // max_num_merge_cand: spec maximum, matches vulkan::hevc_params's identical choice
        0, // slice_qp_delta: `pic_init_qp` (FIXED_QP) used directly
        0, // slice_cb_qp_offset
        0, // slice_cr_qp_offset
        0, // slice_beta_offset_div2
        0, // slice_tc_offset_div2
        &slice_fields,
        0, // pred_weight_table_bit_offset: unused, no weighted prediction
        0, // pred_weight_table_bit_length
    )
}

/// A `VAPictureHEVC` `ReferenceFrames`/`ref_pic_list0` entry describing one already-encoded
/// picture: `pic_order_cnt` = the referenced picture's `poc`, `flags` =
/// `VA_PICTURE_HEVC_RPS_ST_CURR_BEFORE` — always correct in this design (no reordering, no
/// B-frames, so the sole tracked reference is always temporally before the current picture).
fn reference_picture_hevc(surface_id: cros_libva::VASurfaceID, slot: DpbSlot) -> PictureHEVC {
    PictureHEVC::new(surface_id, slot.poc, VA_PICTURE_HEVC_RPS_ST_CURR_BEFORE)
}

/// An unused `VAPictureHEVC` reference-list slot.
fn invalid_picture_hevc() -> PictureHEVC {
    PictureHEVC::new(VA_INVALID_SURFACE, 0, VA_PICTURE_HEVC_INVALID)
}

/// Coding-tree-unit count for `num_ctu_in_slice` — `CTU_SIZE`-aligned grid, rounding up any
/// non-CTU-aligned remainder (this crate's `validate` only requires 8-pixel/minimum-CB-size
/// alignment, narrower than a full `CTU_SIZE`-pixel alignment).
const fn ctu_count(width: u32, height: u32) -> u32 {
    width.div_ceil(CTU_SIZE) * height.div_ceil(CTU_SIZE)
}

fn validate(config: &VideoEncoderConfig) -> Result<(), EncodeError> {
    // `super::codec::is_supported_video_codec` stays H.264-only (see that function's own doc) —
    // this encoder checks its own codec directly rather than a shared helper.
    if config.codec != CodecKind::Hevc {
        return Err(EncodeError::Unsupported);
    }
    if config.width == 0 || config.height == 0 {
        return Err(EncodeError::InvalidInput);
    }
    // `pic_width_in_luma_samples`/`pic_height_in_luma_samples` are `u16` fields in VA-API's own
    // HEVC encode buffers.
    if config.width > u32::from(u16::MAX) || config.height > u32::from(u16::MAX) {
        return Err(EncodeError::Unsupported);
    }
    // 8-pixel (minimum-CB-size) alignment — mirrors
    // `mediaway-encoder::vulkan::hevc_params::CtuAlignedExtent::from_pixels`'s identical gate,
    // since VA-API itself does not enforce this for us (ADR-0003 § VA-API-specific plumbing).
    if !config.width.is_multiple_of(8) || !config.height.is_multiple_of(8) {
        return Err(EncodeError::Unsupported);
    }
    if config.pixel_format != PixelFormat::Nv12 {
        return Err(EncodeError::Unsupported);
    }
    if config.time_base.den == 0 {
        return Err(EncodeError::InvalidInput);
    }
    if config.gop_size == 0 {
        return Err(EncodeError::InvalidInput);
    }
    Ok(())
}

#[allow(clippy::missing_const_for_fn, reason = "StreamInfo holds Bytes")]
fn stream_info_from(config: &VideoEncoderConfig) -> StreamInfo {
    StreamInfo::Video {
        id: 0,
        codec: CodecKind::Hevc,
        time_base: config.time_base,
        geometry: VideoGeometry {
            width: config.width,
            height: config.height,
        },
        extra_data: Bytes::new(),
    }
}

#[cfg(test)]
#[path = "hevc_tests.rs"]
mod tests;
