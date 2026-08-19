//! VA-API AV1 decode session — `KEY_FRAME`-only, single tile, Main profile, CPU NV12 output.
//!
//! See [ADR-0005](../adr/linux/0005-vaapi-av1-key-frame-decode.md) for scope, the
//! porting-methodology note (this parser is spec-derived, not ported — no AV1 decode
//! precedent exists anywhere else in this workspace), and the **zero real-hardware
//! verification** caveat this backend carries the same as its H.264 siblings.

use std::collections::VecDeque;
use std::rc::Rc;

use crate::{DecodeError, VideoDecoder, VideoDecoderConfig, VideoOutputPreference};
use mediaway_common::{
    Bytes, Packet, PixelFormat, StreamInfo, VideoFrame, VideoFrameStorage, VideoGeometry,
};

use cros_libva::{
    AV1FilmGrain, AV1FilmGrainFields, AV1LoopFilterFields, AV1LoopRestorationFields,
    AV1ModeControlFields, AV1PicInfoFields, AV1QMatrixFields, AV1SegmentInfoFields,
    AV1Segmentation, AV1SeqFields, AV1WarpedMotionParams, BufferType, Config, Context, Display,
    Picture, PictureParameter, PictureParameterBufferAV1, SliceParameter, SliceParameterBufferAV1,
    Surface, VA_FOURCC_NV12, VA_INVALID_ID, VA_RT_FORMAT_YUV420, VAAV1TransformationType,
    VAConfigAttrib, VAConfigAttribType, VAEntrypoint, VAImageFormat,
};

use super::codec::av1_profile_candidates;
use super::nv12::copy_nv12_from_planes;

mod bits;
mod frame_header;
mod obu;
mod sequence_header;
mod tile_info;

use frame_header::FrameHeader;
use obu::{OBU_FRAME, OBU_FRAME_HEADER, OBU_SEQUENCE_HEADER, OBU_TILE_GROUP};
use sequence_header::SequenceHeader;

/// `PRIMARY_REF_NONE` (AV1 spec §3) — always the value for this crate's `KEY_FRAME`-only,
/// `FrameIsIntra`-always scope (`primary_ref_frame` is always spec-inferred to this).
const PRIMARY_REF_NONE: u8 = 7;

/// A decode pipeline bound to one negotiated coded resolution, created lazily the first time a
/// sequence header is available.
struct Av1Pipeline {
    /// Kept alive for the context's lifetime — mirrors this crate's H.264 `Pipeline::_config`.
    _config: Config,
    context: Rc<Context>,
    /// `KEY_FRAME`-only decode references nothing, so a single surface (no DPB ring) suffices —
    /// each `push_packet`-driven decode reuses it.
    surface: Option<Surface<()>>,
    coded_width: u32,
    coded_height: u32,
    nv12_format: VAImageFormat,
}

/// VA-API AV1 decode session. See module docs for scope.
pub(crate) struct VaapiAv1Decoder {
    display: Rc<Display>,
    pipeline: Option<Av1Pipeline>,
    seq: Option<SequenceHeader>,
    /// Bridges a standalone `OBU_FRAME_HEADER` to its following `OBU_TILE_GROUP` (the other
    /// legal AV1 framing besides the combined `OBU_FRAME`) — see [`VaapiAv1Decoder::push_packet`].
    pending_frame_header: Option<FrameHeader>,
    info: StreamInfo,
    declared_width: u32,
    declared_height: u32,
    pending: VecDeque<VideoFrame>,
    flushed: bool,
}

impl VaapiAv1Decoder {
    /// Open per [`VideoDecoderConfig::output`].
    pub(crate) fn open(config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        validate(config)?;
        if config.output != VideoOutputPreference::CpuFramesOk {
            // Zero-Copy DMA-BUF export deferred — see ADR-0001 § Scope (this crate's own
            // standing caveat, unchanged by this ADR).
            return Err(DecodeError::Unsupported);
        }

        let display: Rc<Display> = Display::open().ok_or(DecodeError::Unsupported)?;
        let seq = seed_sequence_header(&config.extra_data);

        Ok(Self {
            display,
            pipeline: None,
            seq,
            pending_frame_header: None,
            info: stream_info_from(config),
            declared_width: config.width,
            declared_height: config.height,
            pending: VecDeque::new(),
            flushed: false,
        })
    }

    /// Create the `Config`/`Surface`/`Context` pipeline on first use. A no-op once created —
    /// dynamic resolution renegotiation mid-session is unsupported this session (matches this
    /// crate's H.264 `ensure_pipeline`).
    fn ensure_pipeline(&mut self, seq: &SequenceHeader) -> Result<(), DecodeError> {
        if self.pipeline.is_some() {
            return Ok(());
        }
        let coded_width = seq.width();
        let coded_height = seq.height();
        if coded_width == 0 || coded_height == 0 {
            return Err(DecodeError::InvalidInput);
        }
        if self.declared_width != 0
            && self.declared_height != 0
            && (coded_width > round_up_16(self.declared_width)
                || coded_height > round_up_16(self.declared_height))
        {
            return Err(DecodeError::InvalidInput);
        }

        let candidates = av1_profile_candidates();
        let supported = self
            .display
            .query_config_profiles()
            .map_err(|_| DecodeError::Backend)?;
        let mut chosen = None;
        for profile in candidates {
            if !supported.contains(&profile) {
                continue;
            }
            let entrypoints = self
                .display
                .query_config_entrypoints(profile)
                .map_err(|_| DecodeError::Backend)?;
            if entrypoints.contains(&VAEntrypoint::VAEntrypointVLD) {
                chosen = Some(profile);
                break;
            }
        }
        let profile = chosen.ok_or(DecodeError::Unsupported)?;

        let attrs = vec![VAConfigAttrib {
            type_: VAConfigAttribType::VAConfigAttribRTFormat,
            value: VA_RT_FORMAT_YUV420,
        }];
        let config = self
            .display
            .create_config(attrs, profile, VAEntrypoint::VAEntrypointVLD)
            .map_err(|_| DecodeError::Backend)?;

        let mut surfaces = self
            .display
            .create_surfaces(
                VA_RT_FORMAT_YUV420,
                Some(VA_FOURCC_NV12),
                coded_width,
                coded_height,
                None,
                vec![()],
            )
            .map_err(|_| DecodeError::Backend)?;

        let context = self
            .display
            .create_context(&config, coded_width, coded_height, Some(&surfaces), true)
            .map_err(|_| DecodeError::Backend)?;

        let surface = surfaces.pop().ok_or(DecodeError::Backend)?;

        let nv12_format = self
            .display
            .query_image_formats()
            .map_err(|_| DecodeError::Backend)?
            .into_iter()
            .find(|f| f.fourcc == VA_FOURCC_NV12)
            .ok_or(DecodeError::Unsupported)?;

        self.pipeline = Some(Av1Pipeline {
            _config: config,
            context,
            surface: Some(surface),
            coded_width,
            coded_height,
            nv12_format,
        });
        Ok(())
    }

    /// Decode one `KEY_FRAME` picture given its already-parsed frame header and raw tile bytes
    /// (the single tile's compressed data, byte-range already resolved by the caller).
    fn decode_picture(
        &mut self,
        packet: &Packet,
        seq: &SequenceHeader,
        fh: &FrameHeader,
        tile_bytes: &[u8],
    ) -> Result<VideoFrame, DecodeError> {
        self.ensure_pipeline(seq)?;
        let pipeline = self.pipeline.as_mut().ok_or(DecodeError::Backend)?;
        let surface = pipeline.surface.take().ok_or(DecodeError::Backend)?;
        let context = Rc::clone(&pipeline.context);
        let coded_width = pipeline.coded_width;
        let coded_height = pipeline.coded_height;
        let nv12_format = pipeline.nv12_format;
        let surface_id = surface.id();

        let outcome = (|| -> Result<(Bytes, Surface<()>), DecodeError> {
            let pic_param = build_pic_param(seq, fh, surface_id);
            let slice_param = build_slice_param(tile_bytes)?;
            let tile_data = tile_bytes.to_vec();

            let pic_param_buf = context
                .create_buffer(BufferType::PictureParameter(PictureParameter::AV1(
                    pic_param,
                )))
                .map_err(|_| DecodeError::Backend)?;
            let slice_param_buf = context
                .create_buffer(BufferType::SliceParameter(SliceParameter::AV1(slice_param)))
                .map_err(|_| DecodeError::Backend)?;
            let slice_data_buf = context
                .create_buffer(BufferType::SliceData(tile_data))
                .map_err(|_| DecodeError::Backend)?;

            let timestamp = u64::try_from(packet.pts).unwrap_or(0);
            let mut picture = Picture::new(timestamp, Rc::clone(&context), surface);
            picture.add_buffer(pic_param_buf);
            picture.add_buffer(slice_param_buf);
            picture.add_buffer(slice_data_buf);

            let picture = picture.begin::<()>().map_err(|_| DecodeError::Backend)?;
            let picture = picture.render().map_err(|_| DecodeError::Backend)?;
            let picture = picture.end().map_err(|_| DecodeError::Backend)?;
            let picture = picture
                .sync::<()>()
                .map_err(|(_e, _pic)| DecodeError::Backend)?;

            let image = picture
                .create_image::<()>(
                    nv12_format,
                    (coded_width, coded_height),
                    (coded_width, coded_height),
                )
                .map_err(|_| DecodeError::Backend)?;
            let va_image = *image.image();
            let bytes = copy_nv12_from_planes(
                image.as_ref(),
                coded_width,
                coded_height,
                va_image.pitches[0],
                va_image.offsets[0],
                va_image.pitches[1],
                va_image.offsets[1],
            );
            drop(image);

            let surface = picture.take_surface().map_err(|_| DecodeError::Backend)?;
            Ok((bytes, surface))
        })();

        match outcome {
            Ok((data, returned_surface)) => {
                if let Some(pipeline) = self.pipeline.as_mut() {
                    pipeline.surface = Some(returned_surface);
                }
                Ok(VideoFrame {
                    pts: packet.pts,
                    duration: packet.duration,
                    width: coded_width,
                    height: coded_height,
                    format: PixelFormat::Nv12,
                    storage: VideoFrameStorage::Cpu { data },
                })
            }
            Err(e) => {
                if let Some(pipeline) = self.pipeline.as_mut() {
                    pipeline.surface = Some(fresh_surface_or_placeholder(
                        &context,
                        coded_width,
                        coded_height,
                    ));
                }
                Err(e)
            }
        }
    }
}

impl VideoDecoder for VaapiAv1Decoder {
    fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    fn push_packet(&mut self, packet: &Packet) -> Result<(), DecodeError> {
        if self.flushed {
            return Err(DecodeError::Closed);
        }
        if packet.is_discard {
            return Ok(());
        }
        let obus = obu::split_obus(packet.payload.as_ref())?;
        for unit in obus {
            match unit.obu_type {
                OBU_SEQUENCE_HEADER => {
                    self.seq = Some(SequenceHeader::parse(unit.payload)?);
                }
                OBU_FRAME_HEADER => {
                    let seq = self.seq.ok_or(DecodeError::InvalidInput)?;
                    self.pending_frame_header = Some(FrameHeader::parse(unit.payload, &seq)?);
                }
                OBU_TILE_GROUP => {
                    let seq = self.seq.ok_or(DecodeError::InvalidInput)?;
                    let fh = self
                        .pending_frame_header
                        .take()
                        .ok_or(DecodeError::InvalidInput)?;
                    // NumTiles == 1 (enforced by tile_info::parse) -> tile_group_obu()'s own
                    // header reads zero bits and the OBU payload starts already byte-aligned,
                    // so the whole payload is this single tile's compressed data.
                    let frame = self.decode_picture(packet, &seq, &fh, unit.payload)?;
                    self.pending.push_back(frame);
                }
                OBU_FRAME => {
                    let seq = self.seq.ok_or(DecodeError::InvalidInput)?;
                    let fh = FrameHeader::parse(unit.payload, &seq)?;
                    // frame_obu()'s own byte_alignment() between the header and the tile group
                    // (AV1 spec §5.10) — round up to the next byte boundary within this OBU's
                    // payload.
                    let tile_start = fh.bits_consumed.div_ceil(8);
                    let tile_bytes = unit
                        .payload
                        .get(tile_start..)
                        .ok_or(DecodeError::InvalidInput)?;
                    let frame = self.decode_picture(packet, &seq, &fh, tile_bytes)?;
                    self.pending.push_back(frame);
                }
                // OBU_TEMPORAL_DELIMITER / OBU_METADATA / OBU_REDUNDANT_FRAME_HEADER /
                // OBU_PADDING / OBU_TILE_LIST are ignored — matches this crate's H.264 path's
                // SEI/AUD disposition.
                _ => {}
            }
        }
        Ok(())
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        Ok(self.pending.pop_front())
    }

    fn flush(&mut self) -> Result<(), DecodeError> {
        // Every push_packet already runs its decode synchronously (vaSyncSurface before
        // returning) — no pending driver pipeline to drain, matches this crate's H.264 flush.
        self.flushed = true;
        Ok(())
    }
}

/// Build `VADecPictureParameterBufferAV1` for one `KEY_FRAME`, every reference slot invalid
/// (a `KEY_FRAME` references nothing) — matches this crate's H.264 IDR-only convention. Every
/// optional-tool sub-struct (`seg_info`/`film_grain_info`/`loop_restoration_fields`/`wm`) is
/// still built as a real, all-disabled struct: `cros-libva`'s constructor takes them as
/// mandatory, non-`Option` parameters even when the corresponding sequence-header flag is off
/// (see ADR-0003 § VA-API-specific plumbing).
#[allow(
    clippy::too_many_lines,
    reason = "one flat field-by-field construction of every VADecPictureParameterBufferAV1 \
              sub-struct (seq_fields/pic_info_fields/loop_filter_info_fields/qmatrix_fields/ \
              mode_control_fields/seg_info/film_grain_info/loop_restoration_fields/wm) plus the \
              40-parameter constructor call itself; splitting further would just move a \
              consecutive slice of the same single struct's fields into a same-file helper, \
              mirroring this crate's H.264 decode_one's identical allow/reasoning"
)]
fn build_pic_param(
    seq: &SequenceHeader,
    fh: &FrameHeader,
    surface_id: cros_libva::VASurfaceID,
) -> PictureParameterBufferAV1 {
    let seq_fields = AV1SeqFields::new(
        0, // still_picture
        u32::from(seq.use_128x128_superblock),
        0, // enable_filter_intra (rejected by SequenceHeader::parse when signaled -> always 0)
        0, // enable_intra_edge_filter
        0, // enable_interintra_compound
        0, // enable_masked_compound
        0, // enable_dual_filter
        u32::from(seq.enable_order_hint),
        0, // enable_jnt_comp
        0, // enable_cdef
        0, // mono_chrome
        u32::from(seq.color_range),
        1, // subsampling_x: 4:2:0 always (seq_profile == 0, enforced)
        1, // subsampling_y
        u32::from(seq.chroma_sample_position),
        0, // film_grain_params_present
    );

    let pic_info_fields = AV1PicInfoFields::new(
        0, // frame_type: KEY_FRAME
        1, // show_frame
        0, // showable_frame: inferred 0 for a shown KEY_FRAME
        1, // error_resilient_mode: inferred 1 for a shown KEY_FRAME
        u32::from(fh.disable_cdf_update),
        u32::from(fh.allow_screen_content_tools),
        1, // force_integer_mv: FrameIsIntra always forces this to 1
        u32::from(fh.allow_intrabc),
        0, // use_superres: enable_superres is always rejected when signaled
        0, // allow_high_precision_mv: unused for an intra frame
        0, // is_motion_mode_switchable: unused for an intra frame
        0, // use_ref_frame_mvs: unused for an intra frame
        u32::from(fh.disable_frame_end_update_cdf),
        u32::from(fh.tile_info.uniform_tile_spacing_flag),
        0, // allow_warped_motion: FrameIsIntra -> inferred 0
        0, // large_scale_tile: this crate never uses the tile-list large-scale-tile mode
    );

    let loop_filter_info_fields = AV1LoopFilterFields::new(
        fh.loop_filter.sharpness,
        u8::from(fh.loop_filter.delta_enabled),
        u8::from(fh.loop_filter.delta_update),
    );

    let qmatrix_fields = AV1QMatrixFields::new(
        u16::from(fh.quantization.using_qmatrix),
        u16::from(fh.quantization.qm_y),
        u16::from(fh.quantization.qm_u),
        u16::from(fh.quantization.qm_v),
    );

    let mode_control_fields = AV1ModeControlFields::new(
        u32::from(fh.delta_q_present),
        u32::from(fh.delta_q_res),
        u32::from(fh.delta_lf_present),
        u32::from(fh.delta_lf_res),
        u32::from(fh.delta_lf_multi),
        fh.tx_mode,
        0, // reference_select: FrameIsIntra -> inferred 0
        u32::from(fh.reduced_tx_set),
        0, // skip_mode_present: FrameIsIntra -> inferred 0
    );

    // segmentation_params()/film_grain_params() are always disabled this scope (rejected by
    // SequenceHeader::parse / FrameHeader::parse when signaled) — real, all-disabled structs,
    // never omitted (see this function's own doc comment).
    let seg_info = AV1Segmentation::new(
        &AV1SegmentInfoFields::new(0, 0, 0, 0),
        [[0i16; 8]; 8],
        [0u8; 8],
    );
    #[allow(
        clippy::too_many_arguments,
        reason = "cros-libva's AV1FilmGrain::new mirrors VAFilmGrainStructAV1 field-for-field"
    )]
    let film_grain_info = AV1FilmGrain::new(
        &AV1FilmGrainFields::new(0, 0, 0, 0, 0, 0, 0, 0),
        0,
        0,
        [0u8; 14],
        [0u8; 14],
        0,
        [0u8; 10],
        [0u8; 10],
        0,
        [0u8; 10],
        [0u8; 10],
        [0i8; 24],
        [0i8; 25],
        [0i8; 25],
        0,
        0,
        0,
        0,
        0,
        0,
    );
    // lr_params(): enable_restoration is always rejected when signaled -> RESTORE_NONE (0) on
    // every plane, matching global_motion_params()'s "returns immediately" no-op for an intra
    // frame reflected in the identity `wm` array below.
    let loop_restoration_fields = AV1LoopRestorationFields::new(0, 0, 0, 0, 0);
    // global_motion_params(): FrameIsIntra -> every reference's warp model stays identity,
    // never read from the bitstream — a fixed 7-element array, not stored on FrameHeader.
    let wm: [AV1WarpedMotionParams; 7] = std::array::from_fn(|_| {
        AV1WarpedMotionParams::new(
            VAAV1TransformationType::VAAV1TransformationIdentity,
            [0i32; 8],
            0,
        )
    });

    let mut width_in_sbs_minus_1 = [0u16; 63];
    width_in_sbs_minus_1[0] = u16::try_from(fh.tile_info.sb_cols.saturating_sub(1)).unwrap_or(0);
    let mut height_in_sbs_minus_1 = [0u16; 63];
    height_in_sbs_minus_1[0] = u16::try_from(fh.tile_info.sb_rows.saturating_sub(1)).unwrap_or(0);

    let order_hint_bits_minus_1 = u8::try_from(seq.order_hint_bits.saturating_sub(1)).unwrap_or(0);

    #[allow(
        clippy::too_many_arguments,
        reason = "cros-libva's PictureParameterBufferAV1::new mirrors VADecPictureParameterBufferAV1 field-for-field"
    )]
    PictureParameterBufferAV1::new(
        seq.seq_profile,
        order_hint_bits_minus_1,
        0, // bit_depth_idx: 8-bit only (high_bitdepth is always rejected when signaled)
        seq.matrix_coefficients,
        &seq_fields,
        surface_id,
        surface_id, // current_display_picture == current_frame: no film grain this scope
        Vec::new(), // anchor_frames_list: large-scale-tile feature, unused
        fh.frame_width_minus1,
        fh.frame_height_minus1,
        0,                  // output_frame_width_in_tiles_minus_1: large-scale-tile feature, unused
        0,                  // output_frame_height_in_tiles_minus_1
        [VA_INVALID_ID; 8], // ref_frame_map: a KEY_FRAME references nothing
        [0u8; 7], // ref_frame_idx: harmless, never dereferenced when ref_frame_map is invalid
        PRIMARY_REF_NONE,
        fh.order_hint,
        &seg_info,
        &film_grain_info,
        1, // tile_cols: enforced single-tile by tile_info::parse
        1, // tile_rows
        width_in_sbs_minus_1,
        height_in_sbs_minus_1,
        0, // tile_count_minus_1: NumTiles - 1 == 0
        fh.tile_info.context_update_tile_id,
        &pic_info_fields,
        8, // superres_scale_denominator: SUPERRES_NUM, no scaling (enable_superres rejected)
        0, // interp_filter: unused for an intra frame
        fh.loop_filter.level,
        fh.loop_filter.level_u,
        fh.loop_filter.level_v,
        &loop_filter_info_fields,
        fh.loop_filter.ref_deltas,
        fh.loop_filter.mode_deltas,
        fh.quantization.base_q_idx,
        fh.quantization.delta_q_y_dc,
        fh.quantization.delta_q_u_dc,
        fh.quantization.delta_q_u_ac,
        fh.quantization.delta_q_v_dc,
        fh.quantization.delta_q_v_ac,
        &qmatrix_fields,
        &mode_control_fields,
        0,        // cdef_damping_minus_3: enable_cdef is always rejected when signaled
        0,        // cdef_bits
        [0u8; 8], // cdef_y_strengths
        [0u8; 8], // cdef_uv_strengths
        &loop_restoration_fields,
        &wm,
    )
}

/// Build `VASliceParameterBufferAV1` for this crate's single-tile scope: exactly one
/// `add_slice_parameter` call, `slice_data_offset == 0` (the `SliceData` buffer *is* this
/// tile), `tile_row == tile_column == tg_start == tg_end == 0`.
fn build_slice_param(tile_bytes: &[u8]) -> Result<SliceParameterBufferAV1, DecodeError> {
    let slice_data_size = u32::try_from(tile_bytes.len()).map_err(|_| DecodeError::InvalidInput)?;
    let mut params = SliceParameterBufferAV1::new();
    params.add_slice_parameter(
        slice_data_size,
        0, // slice_data_offset
        0, // slice_data_flag = VA_SLICE_DATA_FLAG_ALL
        0, // tile_row
        0, // tile_column
        0, // tg_start
        0, // tg_end
        0, // anchor_frame_idx: large-scale-tile feature, unused
        0, // tile_idx_in_tile_list: large-scale-tile feature, unused
    );
    Ok(params)
}

/// After a `Picture` consumes its surface, this crate cannot recover the exact same
/// [`Surface`] object on any of `decode_picture`'s error paths — mirrors this crate's H.264
/// `fresh_surface_or_placeholder` rationale exactly.
fn fresh_surface_or_placeholder(
    context: &Rc<Context>,
    coded_width: u32,
    coded_height: u32,
) -> Surface<()> {
    let display = context.display();
    display
        .create_surfaces(
            VA_RT_FORMAT_YUV420,
            Some(VA_FOURCC_NV12),
            coded_width,
            coded_height,
            None,
            vec![()],
        )
        .ok()
        .and_then(|mut v| v.pop())
        .unwrap_or_else(|| placeholder_surface(display, coded_width, coded_height))
}

/// Last-resort placeholder when even a minimal surface allocation fails — mirrors this crate's
/// H.264 `placeholder_surface` exactly (session is already unusable at that point).
fn placeholder_surface(display: &Rc<Display>, coded_width: u32, coded_height: u32) -> Surface<()> {
    #[allow(
        clippy::expect_used,
        reason = "session is already unrecoverable by this point; see caller doc comment"
    )]
    display
        .create_surfaces(
            VA_RT_FORMAT_YUV420,
            Some(VA_FOURCC_NV12),
            coded_width,
            coded_height,
            None,
            vec![()],
        )
        .expect("VA-API display known-good at session open time")
        .pop()
        .expect("exactly one surface requested")
}

/// Best-effort seed of the sequence header from `extra_data` at `open()` time — parse failures
/// here are not fatal since an in-band `OBU_SEQUENCE_HEADER` from `push_packet` can seed it
/// instead. Mirrors this crate's H.264 `seed_params`.
fn seed_sequence_header(extra_data: &Bytes) -> Option<SequenceHeader> {
    if extra_data.is_empty() {
        return None;
    }
    let obus = obu::split_obus(extra_data.as_ref()).ok()?;
    obus.into_iter()
        .find(|o| o.obu_type == OBU_SEQUENCE_HEADER)
        .and_then(|o| SequenceHeader::parse(o.payload).ok())
}

fn validate(config: &VideoDecoderConfig) -> Result<(), DecodeError> {
    // This decoder only ever handles AV1 — an H.264 config must route to `VaapiH264Decoder`
    // instead (see that decoder's own `validate` for the mirrored reasoning).
    if config.codec != mediaway_common::CodecKind::Av1 {
        return Err(DecodeError::Unsupported);
    }
    if config.pixel_format != PixelFormat::Nv12 {
        return Err(DecodeError::Unsupported);
    }
    if config.time_base.den == 0 {
        return Err(DecodeError::InvalidInput);
    }
    Ok(())
}

const fn round_up_16(value: u32) -> u32 {
    value.div_ceil(16) * 16
}

#[allow(clippy::missing_const_for_fn, reason = "StreamInfo holds Bytes")]
fn stream_info_from(config: &VideoDecoderConfig) -> StreamInfo {
    StreamInfo::Video {
        id: 0,
        codec: config.codec,
        time_base: config.time_base,
        geometry: VideoGeometry {
            width: config.width,
            height: config.height,
        },
        extra_data: config.extra_data.clone(), // clone: owned StreamInfo snapshot at open
    }
}

#[cfg(test)]
#[path = "av1_tests.rs"]
mod tests;
