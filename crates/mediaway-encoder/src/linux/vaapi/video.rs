//! VA-API H.264 CPU-upload encode session.
//!
//! See [ADR-0001](../../adr/0001-vaapi-cros-libva-h264-cpu-upload.md): binding choice, scope
//! (Constrained Baseline / CQP / all-IDR / CPU upload only), and the zero-hardware-
//! verification caveat for this crate as authored.

use std::collections::VecDeque;
use std::rc::Rc;

use crate::{EncodeError, VideoEncoder, VideoEncoderConfig, VideoInputPreference};
use mediaway_common::{
    Bytes, CodecKind, Packet, PixelFormat, StreamInfo, VideoFrame, VideoFrameStorage, VideoGeometry,
};

use cros_libva::{
    BufferType, Config, Context, Display, EncPictureParameter, EncSequenceParameter,
    EncSliceParameter, H264EncPicFields, H264EncSeqFields, Image, MappedCodedBuffer, Picture,
    PictureH264, Surface, UsageHint, VA_FOURCC_NV12, VA_INVALID_ID, VA_LSB_FIRST, VA_RC_CQP,
    VA_RT_FORMAT_YUV420, VAConfigAttrib, VAConfigAttribType, VAEntrypoint, VAImageFormat,
};

use super::codec::video_profile;

/// Number of driver surfaces kept in rotation. Every frame is an independent IDR (no
/// reference frames held across calls — see ADR-0001), so a small ring is enough to let the
/// driver finish reading one surface's `Image` while we upload into another; there is no
/// deeper pipelining in this stage since `push_frame` runs the whole encode synchronously.
const SURFACE_POOL_SIZE: usize = 4;

/// Fixed quantization parameter used under `VA_RC_CQP` (range 0..=51). Mid-range value;
/// this stage does not expose a quality/bitrate knob — see ADR-0001 § Scope.
const FIXED_QP: u8 = 26;

/// H.264 Level 3.0 (`level_idc` value, i.e. level * 10) — generous for small CQP test frames.
const LEVEL_IDC: u8 = 30;

/// VA-API H.264 encode session (Constrained Baseline, CQP, CPU NV12 upload, all-IDR).
pub(crate) struct VaapiVideoEncoder {
    context: Rc<Context>,
    /// Kept alive for the context's lifetime (`vaCreateContext` reads it at creation time,
    /// but VA-API does not document that the config may be destroyed immediately after —
    /// keep it alive defensively for the whole session).
    _config: Config,
    info: StreamInfo,
    width: u32,
    height: u32,
    mb_width: u16,
    mb_height: u16,
    nv12_bytes: usize,
    bits_per_second: u32,
    surfaces: Vec<Option<Surface<()>>>,
    next_surface: usize,
    pending: VecDeque<Packet>,
    flushed: bool,
}

impl VaapiVideoEncoder {
    /// Open according to [`VideoEncoderConfig::input`].
    pub(crate) fn open(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        validate(config)?;
        match config.input {
            VideoInputPreference::CpuUploadOk => Self::open_cpu(config),
            // DMA-BUF Zero-Copy surface import is deferred — see ADR-0001 § Scope / roadmap.
            _ => Err(EncodeError::Unsupported),
        }
    }

    fn open_cpu(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        // `Display::open()` tries `/dev/dri/renderD128..` in order (cros_libva's
        // `DrmDeviceIterator`) and returns the first device where `vaGetDisplayDRM` +
        // `vaInitialize` both succeed. In this session's environment (no real VA-API
        // device), this honestly returns `None` — see ADR-0001's hardware caveat.
        let display: Rc<Display> = Display::open().ok_or(EncodeError::Backend)?;

        let profile = video_profile(config.codec)?;
        let attrs = vec![VAConfigAttrib {
            type_: VAConfigAttribType::VAConfigAttribRateControl,
            value: VA_RC_CQP,
        }];
        let vaconfig = display
            .create_config(attrs, profile, VAEntrypoint::VAEntrypointEncSlice)
            .map_err(|_| EncodeError::Backend)?;

        let mb_width = mb_count(config.width)?;
        let mb_height = mb_count(config.height)?;

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

        let nv12_bytes = nv12_size(config.width, config.height)?;

        Ok(Self {
            context,
            _config: vaconfig,
            info: stream_info_from(config),
            width: config.width,
            height: config.height,
            mb_width,
            mb_height,
            nv12_bytes,
            bits_per_second: config.bitrate_bps,
            surfaces: surfaces.into_iter().map(Some).collect(),
            next_surface: 0,
            pending: VecDeque::new(),
            flushed: false,
        })
    }

    /// Encode one frame from `surface` (already holding the uploaded NV12 picture).
    ///
    /// Returns the surface to give back to the pool slot, when one is recoverable, alongside
    /// the encode result. `Picture::begin`/`render`/`end` take their receiver by value and
    /// only give it back embedded in the next typestate on success, so a failure at those
    /// steps loses the surface for good (the `VaError` they return does not carry it); this
    /// is reported as `None` rather than fabricating or panicking to produce a replacement —
    /// see [`Self::push_frame`], which tolerates a pool slot staying empty.
    fn encode_one(
        &self,
        surface: Surface<()>,
        frame: &VideoFrame,
    ) -> (Option<Surface<()>>, Result<Packet, EncodeError>) {
        let num_macroblocks = u32::from(self.mb_width) * u32::from(self.mb_height);

        // Generous size hint: NV12 CQP I-frame output very rarely approaches raw size, but
        // there is no harm in headroom (VA-API resizes if the driver reports overflow via
        // `MappedCodedSegment::status`, which we currently just surface as-is downstream).
        let coded_size = self.nv12_bytes.saturating_mul(2).max(4096);

        let Ok(coded_buf) = self.context.create_enc_coded(coded_size) else {
            return (Some(surface), Err(EncodeError::Backend));
        };

        let surface_id = surface.id();
        let seq_params = build_seq_params(self.mb_width, self.mb_height, self.bits_per_second);
        let pic_params = build_pic_params(surface_id, coded_buf.id());
        let slice_params = build_slice_params(num_macroblocks);

        let Ok(seq_buf) = self.context.create_buffer(BufferType::EncSequenceParameter(
            EncSequenceParameter::H264(seq_params),
        )) else {
            return (Some(surface), Err(EncodeError::Backend));
        };
        let Ok(pic_buf) =
            self.context
                .create_buffer(BufferType::EncPictureParameter(EncPictureParameter::H264(
                    pic_params,
                )))
        else {
            return (Some(surface), Err(EncodeError::Backend));
        };
        let Ok(slice_buf) =
            self.context
                .create_buffer(BufferType::EncSliceParameter(EncSliceParameter::H264(
                    slice_params,
                )))
        else {
            return (Some(surface), Err(EncodeError::Backend));
        };

        let timestamp = u64::try_from(frame.pts).unwrap_or(0);
        let mut picture = Picture::new::<()>(timestamp, Rc::clone(&self.context), surface);
        picture.add_buffer(seq_buf);
        picture.add_buffer(pic_buf);
        picture.add_buffer(slice_buf);

        // `begin`/`render`/`end` return only `VaError` on failure — the surface moved into
        // `picture` is unrecoverable past this point on those branches (see doc comment).
        let Ok(picture) = picture.begin::<()>() else {
            return (None, Err(EncodeError::Backend));
        };
        let Ok(picture) = picture.render() else {
            return (None, Err(EncodeError::Backend));
        };
        let Ok(picture) = picture.end() else {
            return (None, Err(EncodeError::Backend));
        };
        // `sync`'s error arm gives the picture back as `Picture<PictureEnd, T>`, but only
        // `PictureNew`/`PictureSync` implement `PictureReclaimableSurface` — `PictureEnd`
        // does not, so there is no typestate-sanctioned way to pull the surface back out
        // here either; it drops along with the picture, same as the begin/render/end arms.
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
            is_keyframe: true,
            is_discard: false,
            payload: Bytes::from(bytes),
        };
        (surface, Ok(packet))
    }
}

impl VideoEncoder for VaapiVideoEncoder {
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

        let slot = self.next_surface;
        self.next_surface = (self.next_surface + 1) % self.surfaces.len();
        let surface = self.surfaces[slot].take().ok_or(EncodeError::Backend)?;

        let surface = match upload_cpu_nv12(&surface, data, self.width, self.height) {
            Ok(()) => surface,
            Err(e) => {
                self.surfaces[slot] = Some(surface);
                return Err(e);
            }
        };

        let (returned_surface, result) = self.encode_one(surface, frame);
        // May leave this slot `None` when the surface was unrecoverably consumed by a failed
        // `Picture` step (see `encode_one`'s doc comment) — the next `push_frame` call simply
        // rotates past it; `take().ok_or(EncodeError::Backend)` above already handles a `None`
        // slot honestly rather than panicking.
        self.surfaces[slot] = returned_surface;
        let packet = result?;
        self.pending.push_back(packet);
        Ok(())
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        Ok(self.pending.pop_front())
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        // Every `push_frame` already runs its encode synchronously (`vaSyncSurface` before
        // returning) — there is no pending driver pipeline to drain, unlike the Windows MFT
        // path. `flush` only needs to close the session against further pushes.
        self.flushed = true;
        Ok(())
    }
}

/// Copy CPU NV12 bytes into `surface` (`vaCreateImage` + `vaGetImage`, memcpy, `vaPutImage`
/// on drop) — a genuine CPU→driver copy, named to match the Windows backend's
/// `upload_cpu_nv12` cost-disclosure convention. `data` must be tightly packed NV12
/// (`width * height` Y bytes followed by `width * height / 2` interleaved UV bytes).
fn upload_cpu_nv12(
    surface: &Surface<()>,
    data: &[u8],
    width: u32,
    height: u32,
) -> Result<(), EncodeError> {
    let format = VAImageFormat {
        fourcc: VA_FOURCC_NV12,
        byte_order: VA_LSB_FIRST,
        bits_per_pixel: 12,
        depth: 0,
        red_mask: 0,
        green_mask: 0,
        blue_mask: 0,
        alpha_mask: 0,
        va_reserved: Default::default(),
    };
    let mut image = Image::create_from(surface, format, (width, height), (width, height))
        .map_err(|_| EncodeError::Backend)?;
    let va_image = *image.image();
    let offsets = va_image.offsets;
    let pitches = va_image.pitches;

    let w = width as usize;
    let h = height as usize;
    let y_pitch = pitches[0] as usize;
    let y_offset = offsets[0] as usize;
    let uv_pitch = pitches[1] as usize;
    let uv_offset = offsets[1] as usize;
    let y_plane_bytes = w * h;
    let uv_rows = h / 2;

    let dst = image.as_mut();
    if dst.len() < y_offset + y_pitch * h || dst.len() < uv_offset + uv_pitch * uv_rows {
        return Err(EncodeError::Backend);
    }

    for row in 0..h {
        let src = row * w;
        let dst_off = y_offset + row * y_pitch;
        dst[dst_off..dst_off + w].copy_from_slice(&data[src..src + w]);
    }
    for row in 0..uv_rows {
        let src = y_plane_bytes + row * w;
        let dst_off = uv_offset + row * uv_pitch;
        dst[dst_off..dst_off + w].copy_from_slice(&data[src..src + w]);
    }

    Ok(())
    // `image` drops here: `as_mut()` marked it dirty, so `Drop` issues `vaPutImage` to push
    // these bytes into `surface`, then unmaps and destroys the temporary `VAImage`.
}

/// Sequence parameter set: sent on every frame (every frame is an independent IDR — see
/// `build_pic_params`), `pic_order_cnt_type = 2` so no explicit POC bookkeeping is needed.
fn build_seq_params(
    mb_width: u16,
    mb_height: u16,
    bits_per_second: u32,
) -> cros_libva::EncSequenceParameterBufferH264 {
    let seq_fields = H264EncSeqFields::new(
        1, // chroma_format_idc: 4:2:0
        1, // frame_mbs_only_flag: progressive only
        0, // mb_adaptive_frame_field_flag
        0, // seq_scaling_matrix_present_flag
        1, // direct_8x8_inference_flag
        4, // log2_max_frame_num_minus4 (frame_num always 0 in this stage; ample range)
        2, // pic_order_cnt_type = 2: POC derived from frame_num, no explicit fields needed
        0, // log2_max_pic_order_cnt_lsb_minus4 (unused, pic_order_cnt_type != 0)
        0, // delta_pic_order_always_zero_flag (unused, pic_order_cnt_type != 1)
    );

    cros_libva::EncSequenceParameterBufferH264::new(
        0, // seq_parameter_set_id
        LEVEL_IDC,
        1, // intra_period: every frame is an I frame
        1, // intra_idr_period: every frame is an IDR
        0, // ip_period: no P frames in this stage
        bits_per_second,
        1, // max_num_ref_frames: no references kept, but VA-API expects >= 1
        mb_width,
        mb_height,
        &seq_fields,
        0,           // bit_depth_luma_minus8: 8-bit
        0,           // bit_depth_chroma_minus8: 8-bit
        0,           // num_ref_frames_in_pic_order_cnt_cycle (unused, pic_order_cnt_type != 1)
        0,           // offset_for_non_ref_pic (unused, pic_order_cnt_type != 1)
        0,           // offset_for_top_to_bottom_field (unused, pic_order_cnt_type != 1)
        [0i32; 256], // offset_for_ref_frame (unused, pic_order_cnt_type != 1)
        None,        // frame_crop: dimensions are macroblock-aligned already (see `validate`)
        None,        // vui_fields: not needed for this stage's minimal bitstream
        0,           // aspect_ratio_idc
        0,           // sar_width
        0,           // sar_height
        0,           // num_units_in_tick
        0,           // time_scale
    )
}

/// Picture parameter set: every frame is `idr_pic_flag = 1`, `frame_num = 0` — see module doc.
fn build_pic_params(
    surface_id: cros_libva::VASurfaceID,
    coded_buf_id: cros_libva::VABufferID,
) -> cros_libva::EncPictureParameterBufferH264 {
    let curr_pic = PictureH264::new(surface_id, 0, 0, 0, 0);
    let reference_frames: [PictureH264; 16] = std::array::from_fn(|_| invalid_picture_h264());

    let pic_fields = H264EncPicFields::new(
        1, // idr_pic_flag
        0, // reference_pic_flag: not kept as a reference (independent IDRs, ADR-0001)
        // Constrained Baseline profile forbids CABAC — CAVLC only.
        0, // entropy_coding_mode_flag: 0 = CAVLC
        0, // weighted_pred_flag
        0, // weighted_bipred_idc
        1, // constrained_intra_pred_flag
        0, // transform_8x8_mode_flag: Baseline has no 8x8 transform
        0, // deblocking_filter_control_present_flag
        0, // redundant_pic_cnt_present_flag
        0, // pic_order_present_flag: unused, `pic_order_cnt_type == 2`
        0, // pic_scaling_matrix_present_flag
    );

    cros_libva::EncPictureParameterBufferH264::new(
        curr_pic,
        reference_frames,
        coded_buf_id,
        0, // pic_parameter_set_id
        0, // seq_parameter_set_id
        0, // last_picture: no end-of-sequence/stream signaling this stage
        0, // frame_num: always 0 (every frame independent IDR)
        FIXED_QP,
        0, // num_ref_idx_l0_active_minus1 (unused: no P slices)
        0, // num_ref_idx_l1_active_minus1 (unused: no B slices)
        0, // chroma_qp_index_offset
        0, // second_chroma_qp_index_offset
        &pic_fields,
    )
}

/// Single-slice-per-frame slice parameters covering the whole picture.
fn build_slice_params(num_macroblocks: u32) -> cros_libva::EncSliceParameterBufferH264 {
    let ref_pic_list_0: [PictureH264; 32] = std::array::from_fn(|_| invalid_picture_h264());
    let ref_pic_list_1: [PictureH264; 32] = std::array::from_fn(|_| invalid_picture_h264());

    cros_libva::EncSliceParameterBufferH264::new(
        0, // macroblock_address: whole frame in one slice
        num_macroblocks,
        VA_INVALID_ID, // macroblock_info: no per-macroblock override
        2,             // slice_type: I slice
        0,             // pic_parameter_set_id
        0,             // idr_pic_id
        0,             // pic_order_cnt_lsb (unused, pic_order_cnt_type == 2)
        0,             // delta_pic_order_cnt_bottom (unused, pic_order_cnt_type == 2)
        [0i32; 2],     // delta_pic_order_cnt (unused, pic_order_cnt_type == 2)
        0,             // direct_spatial_mv_pred_flag (unused: I slice)
        0,             // num_ref_idx_active_override_flag (unused: I slice)
        0,             // num_ref_idx_l0_active_minus1 (unused: I slice)
        0,             // num_ref_idx_l1_active_minus1 (unused: I slice)
        ref_pic_list_0,
        ref_pic_list_1,
        0, // luma_log2_weight_denom
        0, // chroma_log2_weight_denom
        0, // luma_weight_l0_flag
        [0i16; 32],
        [0i16; 32],
        0, // chroma_weight_l0_flag
        [[0i16; 2]; 32],
        [[0i16; 2]; 32],
        0, // luma_weight_l1_flag
        [0i16; 32],
        [0i16; 32],
        0, // chroma_weight_l1_flag
        [[0i16; 2]; 32],
        [[0i16; 2]; 32],
        0, // cabac_init_idc (unused: CAVLC)
        0, // slice_qp_delta: `pic_init_qp` (FIXED_QP) used directly
        0, // disable_deblocking_filter_idc: filter enabled
        0, // slice_alpha_c0_offset_div2
        0, // slice_beta_offset_div2
    )
}

/// An unused `VAPictureH264` DPB / reference-list slot.
fn invalid_picture_h264() -> PictureH264 {
    PictureH264::new(
        cros_libva::VA_INVALID_SURFACE,
        0,
        cros_libva::VA_PICTURE_H264_INVALID,
        0,
        0,
    )
}

fn validate(config: &VideoEncoderConfig) -> Result<(), EncodeError> {
    if !super::codec::is_supported_video_codec(config.codec) {
        return Err(EncodeError::Unsupported);
    }
    if config.width == 0 || config.height == 0 {
        return Err(EncodeError::InvalidInput);
    }
    // Non-macroblock-aligned resolutions need SPS frame-cropping fields, out of scope here
    // (ADR-0001 § Scope).
    if !config.width.is_multiple_of(16) || !config.height.is_multiple_of(16) {
        return Err(EncodeError::Unsupported);
    }
    if config.pixel_format != PixelFormat::Nv12 {
        return Err(EncodeError::Unsupported);
    }
    if config.time_base.den == 0 {
        return Err(EncodeError::InvalidInput);
    }
    Ok(())
}

fn mb_count(dim: u32) -> Result<u16, EncodeError> {
    u16::try_from(dim / 16).map_err(|_| EncodeError::InvalidInput)
}

fn nv12_size(width: u32, height: u32) -> Result<usize, EncodeError> {
    let w = usize::try_from(width).map_err(|_| EncodeError::InvalidInput)?;
    let h = usize::try_from(height).map_err(|_| EncodeError::InvalidInput)?;
    w.checked_mul(h)
        .and_then(|y| y.checked_mul(3))
        .and_then(|v| v.checked_div(2))
        .ok_or(EncodeError::InvalidInput)
}

#[allow(clippy::missing_const_for_fn, reason = "StreamInfo holds Bytes")]
fn stream_info_from(config: &VideoEncoderConfig) -> StreamInfo {
    StreamInfo::Video {
        id: 0,
        codec: CodecKind::H264,
        time_base: config.time_base,
        geometry: VideoGeometry {
            width: config.width,
            height: config.height,
        },
        extra_data: Bytes::new(),
    }
}

#[cfg(test)]
#[path = "video_tests.rs"]
mod tests;
