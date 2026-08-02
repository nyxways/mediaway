//! VA-API H.264 decode session (IDR pictures only, single slice per picture, progressive,
//! `pic_order_cnt_type == 0`, CPU NV12 output).
//!
//! See [ADR-0001](../../adr/0001-vaapi-h264-cpu-out.md): binding choice, scope, and the
//! zero-hardware-verification caveat for this crate as authored.

use std::collections::VecDeque;
use std::rc::Rc;

use mediaway_common::{
    Bytes, Packet, PixelFormat, StreamInfo, VideoFrame, VideoFrameStorage, VideoGeometry,
};
use mediaway_decoder::{DecodeError, VideoDecoder, VideoDecoderConfig, VideoOutputPreference};
use mediaway_sw::h264::{NalUnit, NalUnitType, split_annex_b};

use cros_libva::{
    BufferType, Config, Context, Display, H264PicFields, H264SeqFields, IQMatrix,
    IQMatrixBufferH264, Picture, PictureH264, PictureParameter, PictureParameterBufferH264,
    SliceParameter, SliceParameterBufferH264, Surface, VA_FOURCC_NV12, VA_INVALID_SURFACE,
    VA_PICTURE_H264_INVALID, VA_RT_FORMAT_YUV420, VAConfigAttrib, VAConfigAttribType, VAEntrypoint,
    VAImageFormat,
};

use super::codec::h264_profile_candidates;
use super::nv12::copy_nv12_from_planes;
use super::pps::Pps;
use super::slice::SliceHeader;
use super::sps::Sps;

/// Driver surfaces kept in rotation. Structural parity with `mediaway-encoder-linux`'s
/// identical pool: every picture is decoded fully synchronously within one `push_packet` call
/// (`vaSyncSurface` before this function returns), so pipelining is not exploited yet, but a
/// small ring keeps this crate's shape parallel to its encode sibling.
const SURFACE_POOL_SIZE: usize = 4;

/// A decode pipeline bound to one negotiated profile + coded resolution, created lazily the
/// first time an SPS is available — `open()` cannot know the profile/coded resolution before
/// that (`VideoDecoderConfig::extra_data` "may be empty until first keyframe").
struct Pipeline {
    /// Kept alive for the context's lifetime (VA-API does not document that the config may be
    /// destroyed immediately after `vaCreateContext` — mirrors `mediaway-encoder-linux`'s
    /// identical `_config` field).
    _config: Config,
    context: Rc<Context>,
    surfaces: Vec<Option<Surface<()>>>,
    next_surface: usize,
    coded_width: u32,
    coded_height: u32,
    nv12_format: VAImageFormat,
}

/// VA-API H.264 decode session. See module docs for scope.
pub(crate) struct VaapiH264Decoder {
    display: Rc<Display>,
    pipeline: Option<Pipeline>,
    sps: Option<Sps>,
    pps: Option<Pps>,
    info: StreamInfo,
    declared_width: u32,
    declared_height: u32,
    pending: VecDeque<VideoFrame>,
    flushed: bool,
}

impl VaapiH264Decoder {
    /// Open per [`VideoDecoderConfig::output`].
    pub(crate) fn open(config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        validate(config)?;
        if config.output != VideoOutputPreference::CpuFramesOk {
            // Zero-Copy DMA-BUF export deferred — see ADR-0001 § Scope.
            return Err(DecodeError::Unsupported);
        }

        // `Display::open()` tries `/dev/dri/renderD128..` in order (cros_libva's
        // `DrmDeviceIterator`), wrapping `vaGetDisplayDRM` + `vaInitialize`. In this session's
        // environment (no real VA-API device) this honestly returns `None` — see ADR-0001's
        // hardware caveat.
        let display: Rc<Display> = Display::open().ok_or(DecodeError::Unsupported)?;

        let (sps, pps) = seed_params(&config.extra_data);

        Ok(Self {
            display,
            pipeline: None,
            sps,
            pps,
            info: stream_info_from(config),
            declared_width: config.width,
            declared_height: config.height,
            pending: VecDeque::new(),
            flushed: false,
        })
    }

    /// Create the `Config`/`Surface`/`Context` pipeline on first use. A no-op once created —
    /// dynamic resolution/profile renegotiation mid-session is unsupported this session.
    fn ensure_pipeline(&mut self, sps: Sps) -> Result<(), DecodeError> {
        if self.pipeline.is_some() {
            return Ok(());
        }
        let coded_width = sps.width();
        let coded_height = sps.height();
        if coded_width == 0 || coded_height == 0 {
            return Err(DecodeError::InvalidInput);
        }
        if self.declared_width != 0
            && self.declared_height != 0
            && (coded_width > round_up_16(self.declared_width)
                || coded_height > round_up_16(self.declared_height))
        {
            // Stream reports a picture larger than the caller declared at `open()`; dynamic
            // resolution renegotiation is unsupported this session.
            return Err(DecodeError::InvalidInput);
        }

        let candidates = h264_profile_candidates(sps.profile_idc)?;
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

        let surfaces = self
            .display
            .create_surfaces(
                VA_RT_FORMAT_YUV420,
                Some(VA_FOURCC_NV12),
                coded_width,
                coded_height,
                None,
                vec![(); SURFACE_POOL_SIZE],
            )
            .map_err(|_| DecodeError::Backend)?;

        let context = self
            .display
            .create_context(&config, coded_width, coded_height, Some(&surfaces), true)
            .map_err(|_| DecodeError::Backend)?;

        let nv12_format = self
            .display
            .query_image_formats()
            .map_err(|_| DecodeError::Backend)?
            .into_iter()
            .find(|f| f.fourcc == VA_FOURCC_NV12)
            .ok_or(DecodeError::Unsupported)?;

        self.pipeline = Some(Pipeline {
            _config: config,
            context,
            surfaces: surfaces.into_iter().map(Some).collect(),
            next_surface: 0,
            coded_width,
            coded_height,
            nv12_format,
        });
        Ok(())
    }

    fn take_surface_slot(&mut self) -> Result<(usize, Surface<()>), DecodeError> {
        let pipeline = self.pipeline.as_mut().ok_or(DecodeError::Backend)?;
        let slot = pipeline.next_surface;
        pipeline.next_surface = (pipeline.next_surface + 1) % pipeline.surfaces.len();
        let surface = pipeline.surfaces[slot].take().ok_or(DecodeError::Backend)?;
        Ok((slot, surface))
    }

    /// Decode one IDR picture. Takes `&self` (not `&mut self`) so the caller can hand the
    /// returned surface back into `self.pipeline` afterward without a double mutable borrow —
    /// mirrors `mediaway-encoder-linux`'s `encode_one` shape.
    #[allow(clippy::too_many_arguments)]
    fn decode_one(
        &self,
        surface: Surface<()>,
        packet: &Packet,
        original_nal: &[u8],
        unit: &NalUnit,
        sps: &Sps,
        pps: &Pps,
        header: &SliceHeader,
    ) -> (Surface<()>, Result<VideoFrame, DecodeError>) {
        let Some(pipeline) = self.pipeline.as_ref() else {
            return (surface, Err(DecodeError::Backend));
        };
        let context = Rc::clone(&pipeline.context);
        let coded_width = pipeline.coded_width;
        let coded_height = pipeline.coded_height;
        let nv12_format = pipeline.nv12_format;
        let surface_id = surface.id();

        let outcome = (|| -> Result<(Bytes, Surface<()>), DecodeError> {
            let pic_param = build_pic_param(sps, pps, unit, header, surface_id)?;
            let iq_matrix = IQMatrixBufferH264::new([[16u8; 16]; 6], [[16u8; 64]; 2]);
            let slice_param = build_slice_param(header, original_nal.len())?;
            let slice_data_bytes = original_nal.to_vec();

            let pic_param_buf = context
                .create_buffer(BufferType::PictureParameter(PictureParameter::H264(
                    pic_param,
                )))
                .map_err(|_| DecodeError::Backend)?;
            let iq_buf = context
                .create_buffer(BufferType::IQMatrix(IQMatrix::H264(iq_matrix)))
                .map_err(|_| DecodeError::Backend)?;
            let slice_param_buf = context
                .create_buffer(BufferType::SliceParameter(SliceParameter::H264(
                    slice_param,
                )))
                .map_err(|_| DecodeError::Backend)?;
            let slice_data_buf = context
                .create_buffer(BufferType::SliceData(slice_data_bytes))
                .map_err(|_| DecodeError::Backend)?;

            let timestamp = u64::try_from(packet.pts).unwrap_or(0);
            let mut picture = Picture::new(timestamp, Rc::clone(&context), surface);
            picture.add_buffer(pic_param_buf);
            picture.add_buffer(iq_buf);
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
                let frame = VideoFrame {
                    pts: packet.pts,
                    duration: packet.duration,
                    width: coded_width,
                    height: coded_height,
                    format: PixelFormat::Nv12,
                    storage: VideoFrameStorage::Cpu { data },
                };
                (returned_surface, Ok(frame))
            }
            Err(e) => (
                fresh_surface_or_placeholder(&context, coded_width, coded_height),
                Err(e),
            ),
        }
    }
}

impl VideoDecoder for VaapiH264Decoder {
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
        let nals = split_annex_b(packet.payload.as_ref()).map_err(|_| DecodeError::InvalidInput)?;
        for nal in nals {
            let unit = NalUnit::parse(nal).map_err(|_| DecodeError::InvalidInput)?;
            match unit.unit_type {
                NalUnitType::Sps => {
                    self.sps = Some(Sps::parse(unit.rbsp.as_ref())?);
                }
                NalUnitType::Pps => {
                    self.pps = Some(Pps::parse(unit.rbsp.as_ref())?);
                }
                NalUnitType::IdrSlice => {
                    let sps = self.sps.ok_or(DecodeError::InvalidInput)?;
                    let pps = self.pps.ok_or(DecodeError::InvalidInput)?;
                    let header = SliceHeader::parse(unit.rbsp.as_ref(), unit.ref_idc, &sps, &pps)?;
                    if header.pic_parameter_set_id != pps.pic_parameter_set_id {
                        return Err(DecodeError::InvalidInput);
                    }
                    self.ensure_pipeline(sps)?;
                    let (slot, surface) = self.take_surface_slot()?;
                    let (returned_surface, result) =
                        self.decode_one(surface, packet, nal, &unit, &sps, &pps, &header);
                    if let Some(pipeline) = self.pipeline.as_mut() {
                        pipeline.surfaces[slot] = Some(returned_surface);
                    }
                    let frame = result?;
                    self.pending.push_back(frame);
                }
                // Non-IDR slices (P/B/non-IDR I) are out of scope this session — see
                // ADR-0001 § Scope. SEI/AUD/end-of-sequence/-stream/filler are ignored.
                NalUnitType::NonIdrSlice => return Err(DecodeError::Unsupported),
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
        // returning) — there is no pending driver pipeline to drain, unlike a hardware MFT's
        // async event pump. flush only needs to close the session against further pushes.
        self.flushed = true;
        Ok(())
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::similar_names,
    reason = "pic_init_qp_minus26 / pic_init_qs_minus26 are the ITU-T H.264 spec's own names; VAPictureParameterBufferH264::new takes every field positionally"
)]
fn build_pic_param(
    sps: &Sps,
    pps: &Pps,
    unit: &NalUnit,
    header: &SliceHeader,
    surface_id: cros_libva::VASurfaceID,
) -> Result<PictureParameterBufferH264, DecodeError> {
    let poc = i32::try_from(header.pic_order_cnt_lsb).map_err(|_| DecodeError::InvalidInput)?;
    let curr_pic = PictureH264::new(surface_id, header.frame_num, 0, poc, poc);
    let reference_frames: [PictureH264; 16] = std::array::from_fn(|_| invalid_picture());

    let seq_fields = H264SeqFields::new(
        1, // chroma_format_idc: 4:2:0 (implied by baseline/main profile_idc)
        0, // residual_colour_transform_flag: not present for baseline/main SPS
        u32::from(sps.gaps_in_frame_num_value_allowed_flag),
        1, // frame_mbs_only_flag: progressive only (enforced by Sps::parse)
        0, // mb_adaptive_frame_field_flag: unused, frame_mbs_only_flag == 1
        u32::from(sps.direct_8x8_inference_flag),
        0, // min_luma_bi_pred_size8x8: unused (no B slices this session)
        sps.log2_max_frame_num_minus4,
        sps.pic_order_cnt_type,
        sps.log2_max_pic_order_cnt_lsb_minus4,
        0, // delta_pic_order_always_zero_flag: unused, pic_order_cnt_type == 0
    );
    let pic_fields = H264PicFields::new(
        u32::from(pps.entropy_coding_mode_flag),
        0, // weighted_pred_flag: unused (I slices only)
        0, // weighted_bipred_idc: unused (I slices only)
        0, // transform_8x8_mode_flag: rejected by Pps::parse when the PPS sets it
        0, // field_pic_flag: unused, frame_mbs_only_flag == 1
        u32::from(pps.constrained_intra_pred_flag),
        u32::from(pps.pic_order_present_flag),
        u32::from(pps.deblocking_filter_control_present_flag),
        u32::from(pps.redundant_pic_cnt_present_flag),
        u32::from(unit.ref_idc != 0),
    );

    let pic_width_in_mbs_minus1 =
        u16::try_from(sps.pic_width_in_mbs_minus1).map_err(|_| DecodeError::InvalidInput)?;
    let pic_height_in_mbs_minus1 =
        u16::try_from(sps.pic_height_in_map_units_minus1).map_err(|_| DecodeError::InvalidInput)?;
    let pic_init_qp_minus26 =
        i8::try_from(pps.pic_init_qp_minus26).map_err(|_| DecodeError::InvalidInput)?;
    let pic_init_qs_minus26 =
        i8::try_from(pps.pic_init_qs_minus26).map_err(|_| DecodeError::InvalidInput)?;
    let chroma_qp_index_offset =
        i8::try_from(pps.chroma_qp_index_offset).map_err(|_| DecodeError::InvalidInput)?;
    let second_chroma_qp_index_offset =
        i8::try_from(pps.second_chroma_qp_index_offset).map_err(|_| DecodeError::InvalidInput)?;
    let frame_num = u16::try_from(header.frame_num).map_err(|_| DecodeError::InvalidInput)?;

    Ok(PictureParameterBufferH264::new(
        curr_pic,
        reference_frames,
        pic_width_in_mbs_minus1,
        pic_height_in_mbs_minus1,
        0, // bit_depth_luma_minus8: 8-bit only (baseline/main)
        0, // bit_depth_chroma_minus8: 8-bit only (baseline/main)
        0, // num_ref_frames: none kept (IDR-only, no DPB — see ADR-0001 § Scope)
        &seq_fields,
        // num_slice_groups_minus1 / slice_group_map_type / slice_group_change_rate_minus1:
        // FMO unused — Pps::parse already rejected num_slice_groups_minus1 > 0.
        0,
        0,
        0,
        pic_init_qp_minus26,
        pic_init_qs_minus26,
        chroma_qp_index_offset,
        second_chroma_qp_index_offset,
        &pic_fields,
        frame_num,
    ))
}

fn build_slice_param(
    header: &SliceHeader,
    nal_len: usize,
) -> Result<SliceParameterBufferH264, DecodeError> {
    let ref_pic_list_0: [PictureH264; 32] = std::array::from_fn(|_| invalid_picture());
    let ref_pic_list_1: [PictureH264; 32] = std::array::from_fn(|_| invalid_picture());
    let slice_data_size = u32::try_from(nal_len).map_err(|_| DecodeError::InvalidInput)?;
    // "relative to and includes the NAL unit byte" (ITU-T H.264 VA-API buffer contract) — the
    // NAL header byte itself is 8 bits, then `header.bits_consumed` more (see
    // `SliceHeader::bits_consumed` doc comment).
    let slice_data_bit_offset =
        u16::try_from(8usize + header.bits_consumed).map_err(|_| DecodeError::InvalidInput)?;
    let first_mb_in_slice =
        u16::try_from(header.first_mb_in_slice).map_err(|_| DecodeError::InvalidInput)?;
    let slice_qp_delta =
        i8::try_from(header.slice_qp_delta).map_err(|_| DecodeError::InvalidInput)?;
    let slice_alpha_c0_offset_div2 =
        i8::try_from(header.slice_alpha_c0_offset_div2).map_err(|_| DecodeError::InvalidInput)?;
    let slice_beta_offset_div2 =
        i8::try_from(header.slice_beta_offset_div2).map_err(|_| DecodeError::InvalidInput)?;

    Ok(SliceParameterBufferH264::new(
        slice_data_size,
        0, // slice_data_offset: the SliceData buffer *is* this NAL, offset 0
        0, // slice_data_flag = VA_SLICE_DATA_FLAG_ALL: whole slice is in the buffer
        slice_data_bit_offset,
        first_mb_in_slice,
        header.slice_type,
        0, // direct_spatial_mv_pred_flag: unused (I slice)
        0, // num_ref_idx_l0_active_minus1: unused (I slice)
        0, // num_ref_idx_l1_active_minus1: unused (I slice)
        0, // cabac_init_idc: unused (I slice)
        slice_qp_delta,
        header.disable_deblocking_filter_idc,
        slice_alpha_c0_offset_div2,
        slice_beta_offset_div2,
        ref_pic_list_0,
        ref_pic_list_1,
        0, // luma_log2_weight_denom: unused (no weighted prediction)
        0, // chroma_log2_weight_denom: unused
        0,
        [0i16; 32],
        [0i16; 32],
        0,
        [[0i16; 2]; 32],
        [[0i16; 2]; 32],
        0,
        [0i16; 32],
        [0i16; 32],
        0,
        [[0i16; 2]; 32],
        [[0i16; 2]; 32],
    ))
}

/// An unused `VAPictureH264` DPB / reference-list slot (this crate keeps no DPB — IDR-only
/// decode, see ADR-0001 § Scope).
fn invalid_picture() -> PictureH264 {
    PictureH264::new(VA_INVALID_SURFACE, 0, VA_PICTURE_H264_INVALID, 0, 0)
}

/// After a `Picture` consumes its surface, this crate cannot recover the exact same
/// [`Surface`] object on any of `decode_one`'s error paths (the typestate API only returns it
/// via [`Picture::take_surface`] on the success path) — mirrors
/// `mediaway-encoder-linux`'s identical `fresh_surface_or_placeholder` rationale.
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

/// Last-resort placeholder when even a minimal surface allocation fails (session is already
/// unusable at that point; subsequent `push_packet` calls will fail at `create_buffer`/
/// `begin` anyway). Keeping the pool slot `Some(_)` avoids special-casing an exhausted slot
/// everywhere else in this module.
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

/// Best-effort seed of SPS/PPS from `extra_data` at `open()` time (mirrors
/// `mediaway-decoder-windows`'s best-effort `MF_MT_MPEG_SEQUENCE_HEADER` handling) — parse
/// failures here are not fatal since in-band SPS/PPS from `push_packet` can seed them instead.
fn seed_params(extra_data: &Bytes) -> (Option<Sps>, Option<Pps>) {
    if extra_data.is_empty() {
        return (None, None);
    }
    let Ok(nals) = split_annex_b(extra_data.as_ref()) else {
        return (None, None);
    };
    let mut sps = None;
    let mut pps = None;
    for nal in nals {
        let Ok(unit) = NalUnit::parse(nal) else {
            continue;
        };
        match unit.unit_type {
            NalUnitType::Sps => {
                if let Ok(s) = Sps::parse(unit.rbsp.as_ref()) {
                    sps = Some(s);
                }
            }
            NalUnitType::Pps => {
                if let Ok(p) = Pps::parse(unit.rbsp.as_ref()) {
                    pps = Some(p);
                }
            }
            _ => {}
        }
    }
    (sps, pps)
}

fn validate(config: &VideoDecoderConfig) -> Result<(), DecodeError> {
    if !super::codec::is_supported_video_codec(config.codec) {
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
#[path = "h264_tests.rs"]
mod tests;
