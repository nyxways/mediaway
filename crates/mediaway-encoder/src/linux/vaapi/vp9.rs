//! VA-API VP9 CPU-upload encode session — `KEY_FRAME`-only baseline + single-forward-reference
//! `INTER_FRAME` GOP.
//!
//! See [ADR-0004](../../adr/linux/0004-vaapi-vp9-key-frame-and-inter-gop.md): binding choice
//! (plain `cros-libva` `EncSequenceParameterBufferVP9`/`EncPictureParameterBufferVP9` field
//! bags, no packed-header submission needed — unlike this folder's AV1 sibling), scope, the
//! **narrow real-world VP9 encode driver support** caveat (`FFmpeg`'s own source names only i965
//! as a working driver), and the **zero real-hardware verification** caveat every backend in
//! this folder carries. [`gop`](super::vp9_gop) drives the 2-slot physical ping-pong feeding
//! the buffers built below.

use std::collections::VecDeque;
use std::rc::Rc;

use crate::{EncodeError, VideoEncoder, VideoEncoderConfig, VideoInputPreference};
use mediaway_common::{
    Bytes, CodecKind, Packet, PixelFormat, StreamInfo, VideoFrame, VideoFrameStorage, VideoGeometry,
};

use cros_libva::{
    BufferType, Config, Context, Display, EncPictureParameter, EncSequenceParameter, Image,
    MappedCodedBuffer, Picture, Surface, UsageHint, VA_FOURCC_NV12, VA_INVALID_ID, VA_LSB_FIRST,
    VA_RC_CQP, VA_RT_FORMAT_YUV420, VAConfigAttrib, VAConfigAttribType, VAEntrypoint,
    VAImageFormat, VP9EncPicFlags, VP9EncRefFlags,
};

use super::codec::video_profile;
use super::vp9_gop::{FrameDecision, FrameRequest, GopState, WORKSPACE_PING_PONG_SLOTS};

/// `FFmpeg`'s own real, generic VA-API encode probe order (`libavcodec/vaapi_encode.c`'s
/// `vaapi_encode_entrypoints_normal[]`, cited in this ADR's addendum — the ADR's own original
/// design was a 2-step `EncSlice` → `EncSliceLP` ladder; the addendum corrects this to the real
/// 3-step ladder `FFmpeg` itself uses, with `VAEntrypointEncPicture` confirmed as what VP9 encode
/// actually uses in practice since VP9 has no slice concept at all).
const ENTRYPOINT_PROBE_ORDER: [cros_libva::VAEntrypoint::Type; 3] = [
    VAEntrypoint::VAEntrypointEncSlice,
    VAEntrypoint::VAEntrypointEncPicture,
    VAEntrypoint::VAEntrypointEncSliceLP,
];

/// `FFmpeg`'s own literal, `vaapi_encode_vp9.c`'s `VP9_MAX_TILE_WIDTH`.
const VP9_MAX_TILE_WIDTH: u32 = 4096;

/// Fixed `luma_ac_qindex` for `KEY_FRAME` / `INTER_FRAME` (CQP, this ADR's own scope — no
/// VBR/CBR rate control this pass). Mirrors `FFmpeg`'s own `q_idx_idr`/`q_idx_p` split (cited in
/// the ADR's § VA-API-specific plumbing) — two distinct, arbitrary-but-legal mid-range values,
/// not independently driver-tuned.
const FIXED_QINDEX_KEY: u8 = 60;
const FIXED_QINDEX_INTER: u8 = 80;

/// VA-API VP9 encode session (Profile 0, CQP, CPU NV12 upload, 2-slot physical ping-pong).
pub(crate) struct VaapiVp9Encoder {
    context: Rc<Context>,
    /// Kept alive for the context's lifetime — mirrors this crate's H.264 `_config` field.
    _config: Config,
    info: StreamInfo,
    width: u32,
    height: u32,
    /// `VAEntrypointEncSlice`/`VAEntrypointEncPicture`/`VAEntrypointEncSliceLP` — whichever the
    /// 3-step probe ladder found first (see [`ENTRYPOINT_PROBE_ORDER`]).
    #[allow(
        dead_code,
        reason = "read only at open_cpu time to build the Config; kept for future diagnostics \
                  parity with video.rs's supports_p_frames field"
    )]
    entrypoint: cros_libva::VAEntrypoint::Type,
    nv12_bytes: usize,
    surfaces: [Option<Surface<()>>; WORKSPACE_PING_PONG_SLOTS],
    gop: GopState,
    effective_gop_size: u32,
    /// `EncSequenceParameterBufferVP9` is sent exactly once per session (VP9 has no per-frame
    /// sequence header the way H.264 resends SPS at IDR boundaries — see the ADR's own
    /// § VA-API-specific plumbing) — `true` once the first `push_frame` call has submitted it.
    seq_sent: bool,
    pending: VecDeque<Packet>,
    flushed: bool,
}

impl VaapiVp9Encoder {
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
        let display: Rc<Display> = Display::open().ok_or(EncodeError::Backend)?;

        let profile = video_profile(config.codec)?;

        // "Probe, never assume" (this ADR's own real caveat: VP9 encode driver support is
        // narrow, i965-only per `FFmpeg`'s own comment) — confirm both the profile and an encode
        // entrypoint actually exist before claiming VP9 encode support.
        let supported = display
            .query_config_profiles()
            .map_err(|_| EncodeError::Backend)?;
        if !supported.contains(&profile) {
            return Err(EncodeError::Unsupported);
        }
        let entrypoints = display
            .query_config_entrypoints(profile)
            .map_err(|_| EncodeError::Backend)?;
        let entrypoint = ENTRYPOINT_PROBE_ORDER
            .into_iter()
            .find(|candidate| entrypoints.contains(candidate))
            .ok_or(EncodeError::Unsupported)?;

        let effective_gop_size = if config.gop_size > 1 {
            config.gop_size
        } else {
            1
        };

        let attrs = vec![VAConfigAttrib {
            type_: VAConfigAttribType::VAConfigAttribRateControl,
            value: VA_RC_CQP,
        }];
        let vaconfig = display
            .create_config(attrs, profile, entrypoint)
            .map_err(|_| EncodeError::Backend)?;

        let surfaces = display
            .create_surfaces(
                VA_RT_FORMAT_YUV420,
                Some(VA_FOURCC_NV12),
                config.width,
                config.height,
                Some(UsageHint::USAGE_HINT_ENCODER),
                vec![(); WORKSPACE_PING_PONG_SLOTS],
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

        let mut surfaces_iter = surfaces.into_iter();
        let surfaces_array: [Option<Surface<()>>; WORKSPACE_PING_PONG_SLOTS] =
            std::array::from_fn(|_| surfaces_iter.next());

        Ok(Self {
            context,
            _config: vaconfig,
            info: stream_info_from(config),
            width: config.width,
            height: config.height,
            entrypoint,
            nv12_bytes,
            surfaces: surfaces_array,
            gop: GopState::new(effective_gop_size),
            effective_gop_size,
            seq_sent: false,
            pending: VecDeque::new(),
            flushed: false,
        })
    }

    /// Encode one frame from `surface` per `decision` (`vp9_gop::GopState::decide`'s output),
    /// reading `reference`'s `VASurfaceID` as the sole `LAST_FRAME` when this is an
    /// `INTER_FRAME`. Same "surface may be unrecoverably lost past `Picture::begin`" contract as
    /// this crate's H.264 `encode_one` — see that function's doc comment.
    fn encode_one(
        &mut self,
        surface: Surface<()>,
        frame: &VideoFrame,
        decision: &FrameDecision,
        reference: Option<cros_libva::VASurfaceID>,
    ) -> (Option<Surface<()>>, Result<Packet, EncodeError>) {
        let coded_size = self.nv12_bytes.saturating_mul(2).max(4096);
        let Ok(coded_buf) = self.context.create_enc_coded(coded_size) else {
            return (Some(surface), Err(EncodeError::Backend));
        };

        let surface_id = surface.id();

        // Sent exactly once per session — see `Self::seq_sent`'s doc comment.
        let seq_buf = if self.seq_sent {
            None
        } else {
            let seq_params = build_seq_params(self.width, self.height, self.effective_gop_size);
            match self.context.create_buffer(BufferType::EncSequenceParameter(
                EncSequenceParameter::VP9(seq_params),
            )) {
                Ok(buf) => Some(buf),
                Err(_) => return (Some(surface), Err(EncodeError::Backend)),
            }
        };

        let pic_params = build_pic_params(
            surface_id,
            coded_buf.id(),
            decision,
            reference,
            self.width,
            self.height,
        );
        let Ok(pic_buf) =
            self.context
                .create_buffer(BufferType::EncPictureParameter(EncPictureParameter::VP9(
                    pic_params,
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
        // No `EncSliceParameter` buffer — VP9 encode has no slice concept at all (confirmed:
        // real libva has no `VAEncSliceParameterBufferVP9`, and `cros-libva`'s own
        // `EncSliceParameter` enum has no `VP9` variant — see this ADR's addendum).

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
        self.seq_sent = true;
        let packet = Packet {
            stream_id: 0,
            pts: frame.pts,
            dts: frame.pts,
            duration: frame.duration,
            is_keyframe: decision.is_key,
            is_discard: false,
            payload: Bytes::from(bytes),
        };
        (surface, Ok(packet))
    }
}

impl VideoEncoder for VaapiVp9Encoder {
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

        // Lost-reference-surface guard (mirrors this crate's H.264 `push_frame` step 3) — fail
        // hard before `decision.setup_slot`'s surface is touched, since `GopState::decide`'s
        // bookkeeping already advanced and cannot be un-done.
        let reference = match decision.reference_slot {
            Some(ref_slot) => {
                let ref_surface = self.surfaces[ref_slot]
                    .as_ref()
                    .ok_or(EncodeError::Backend)?;
                Some(ref_surface.id())
            }
            None => None,
        };

        let slot = decision.setup_slot;
        let surface = self.surfaces[slot].take().ok_or(EncodeError::Backend)?;

        let surface = match upload_cpu_nv12(&surface, data, self.width, self.height) {
            Ok(()) => surface,
            Err(e) => {
                self.surfaces[slot] = Some(surface);
                return Err(e);
            }
        };

        let (returned_surface, result) = self.encode_one(surface, frame, &decision, reference);
        self.surfaces[slot] = returned_surface;
        let packet = result?;
        self.pending.push_back(packet);
        Ok(())
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        Ok(self.pending.pop_front())
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        // Every push_frame already runs its encode synchronously (vaSyncSurface before
        // returning) — no pending driver pipeline to drain, matches this crate's H.264 flush.
        self.flushed = true;
        Ok(())
    }
}

/// Copy CPU NV12 bytes into `surface` — same genuine CPU→driver copy as this crate's H.264
/// `upload_cpu_nv12` (see that function's doc comment for the cost disclosure); duplicated
/// rather than shared across codec modules since each caller's `EncodeError` conversion sites
/// differ and the function itself is small.
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
}

/// `EncSequenceParameterBufferVP9` — sent once per session (see `Self::seq_sent`'s doc comment).
/// `kf_auto = 0` (this crate's own `GopState` decides keyframe cadence itself, matching H.264/
/// AV1's identical choice); `bits_per_second = 0` (CQP, unread); `intra_period`/`kf_min_dist`/
/// `kf_max_dist` set defensively from `effective_gop_size` even though `kf_auto = 0` means the
/// driver should not act on them (mirrors this crate's AV1 ADR-0003 defensive-but-inert field
/// convention).
fn build_seq_params(
    width: u32,
    height: u32,
    effective_gop_size: u32,
) -> cros_libva::EncSequenceParameterBufferVP9 {
    cros_libva::EncSequenceParameterBufferVP9::new(
        width,
        height,
        0, // kf_auto
        effective_gop_size,
        effective_gop_size,
        0, // bits_per_second: CQP, unread
        effective_gop_size,
    )
}

/// `EncPictureParameterBufferVP9` + its `ref_flags`/`pic_flags` sub-structs — see the ADR's own
/// § VA-API-specific plumbing for the field-by-field rationale (`bit_offset_*`/
/// `bit_size_segmentation` all `0`, matching the only known real, shipping reference
/// implementation's convention; `ref_lf_delta`/`mode_lf_delta` all-zero, loop-filter deltas
/// disabled).
fn build_pic_params(
    surface_id: cros_libva::VASurfaceID,
    coded_buf_id: cros_libva::VABufferID,
    decision: &FrameDecision,
    reference: Option<cros_libva::VASurfaceID>,
    width: u32,
    height: u32,
) -> cros_libva::EncPictureParameterBufferVP9 {
    let mut reference_frames = [VA_INVALID_ID; 8];
    // Single-forward-reference: only LAST_FRAME (index 0) is ever populated; `ref_last_idx`
    // is always 0 (the sole `reference_frames` array position this ADR's scope ever uses).
    if let Some(ref_surface_id) = reference {
        reference_frames[0] = ref_surface_id;
    }
    let ref_last_idx = 0u32;

    let ref_flags = if decision.is_key {
        VP9EncRefFlags::new(
            1, // force_kf
            0, 0, 0, 0, 0, 0, 0, 0, 0,
        )
    } else {
        VP9EncRefFlags::new(
            0,            // force_kf
            1,            // ref_frame_ctrl_l0: one L0 reference active
            0,            // ref_frame_ctrl_l1
            ref_last_idx, // ref_last_idx
            1,            // ref_last_sign_bias: matches `FFmpeg`'s unconditional choice
            0,
            0,
            0,
            0,
            0, // ref_gf_*/ref_arf_*/temporal_id: unused, LAST_FRAME-only scope
        )
    };

    // reset_frame_context: 3 ("reset all four contexts") on every KEY_FRAME, 0 otherwise — the
    // simplest legal choice (ADR § Scope's own deliberate `error_resilient_mode = 1` framing).
    let reset_frame_context = u32::from(decision.is_key) * 3;
    let pic_flags = VP9EncPicFlags::new(
        u32::from(!decision.is_key), // frame_type: 0 = KEY_FRAME, 1 = INTER_FRAME
        1,                           // show_frame
        1,                           // error_resilient_mode
        0,                           // intra_only
        0,                           // allow_high_precision_mv
        0,                           // mcomp_filter_type: EIGHTTAP
        1,                           // frame_parallel_decoding_mode
        reset_frame_context,
        0, // refresh_frame_context
        0, // frame_context_idx
        0, // segmentation_enabled
        0, // segmentation_temporal_update
        0, // segmentation_update_map
        0, // lossless_mode
        0, // comp_prediction_mode
        0, // auto_segmentation
        0, // super_frame_flag
    );

    cros_libva::EncPictureParameterBufferVP9::new(
        width,      // frame_width_src
        height,     // frame_height_src
        width,      // frame_width_dst: no scaled-reference encode this scope
        height,     // frame_height_dst
        surface_id, // reconstructed_frame: this frame's own destination surface
        reference_frames,
        coded_buf_id,
        &ref_flags,
        &pic_flags,
        decision.refresh_frame_flags,
        if decision.is_key {
            FIXED_QINDEX_KEY
        } else {
            FIXED_QINDEX_INTER
        },
        0,        // luma_dc_qindex_delta
        0,        // chroma_ac_qindex_delta
        0,        // chroma_dc_qindex_delta
        0,        // filter_level: loop filter disabled, simplest legal choice
        0,        // sharpness_level
        [0i8; 4], // ref_lf_delta: disabled
        [0i8; 2], // mode_lf_delta: disabled
        0,
        0,
        0,
        0,
        0,
        0,
        0, // bit_offset_*/bit_size_segmentation: all 0 (see ADR § plumbing)
        0, // log2_tile_rows: not set by `FFmpeg`'s own function, this ADR defaults 0
        log2_tile_columns(width),
        0, // skip_frame_flag
        0, // number_skip_frames
        0, // skip_frames_size
    )
}

fn validate(config: &VideoEncoderConfig) -> Result<(), EncodeError> {
    // This encoder only ever handles VP9 — an H.264 config must route to `VaapiH264Encoder`
    // instead, not silently be accepted here (see `video.rs::validate`'s identical reasoning).
    if config.codec != CodecKind::Vp9 {
        return Err(EncodeError::Unsupported);
    }
    if config.width == 0 || config.height == 0 {
        return Err(EncodeError::InvalidInput);
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

fn nv12_size(width: u32, height: u32) -> Result<usize, EncodeError> {
    let w = usize::try_from(width).map_err(|_| EncodeError::InvalidInput)?;
    let h = usize::try_from(height).map_err(|_| EncodeError::InvalidInput)?;
    w.checked_mul(h)
        .and_then(|y| y.checked_mul(3))
        .and_then(|v| v.checked_div(2))
        .ok_or(EncodeError::InvalidInput)
}

/// `log2_tile_columns` — `FFmpeg`'s own exact formula (`vaapi_encode_vp9.c`, cited in the ADR),
/// reused verbatim: always `0` for every resolution this crate currently accepts (well under
/// `VP9_MAX_TILE_WIDTH = 4096`px wide).
fn log2_tile_columns(frame_width_src: u32) -> u8 {
    let num_tile_columns = frame_width_src.div_ceil(VP9_MAX_TILE_WIDTH).max(1);
    if num_tile_columns == 1 {
        0
    } else {
        let log2 = u32::BITS - (num_tile_columns - 1).leading_zeros();
        u8::try_from(log2).unwrap_or(u8::MAX)
    }
}

#[allow(clippy::missing_const_for_fn, reason = "StreamInfo holds Bytes")]
fn stream_info_from(config: &VideoEncoderConfig) -> StreamInfo {
    StreamInfo::Video {
        id: 0,
        codec: CodecKind::Vp9,
        time_base: config.time_base,
        geometry: VideoGeometry {
            width: config.width,
            height: config.height,
        },
        extra_data: Bytes::new(),
    }
}

#[cfg(test)]
#[path = "vp9_tests.rs"]
mod tests;
