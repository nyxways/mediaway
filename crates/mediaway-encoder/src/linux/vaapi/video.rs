//! VA-API H.264 CPU-upload encode session.
//!
//! See [ADR-0001](../../adr/0001-vaapi-cros-libva-h264-cpu-upload.md): binding choice, scope
//! (Constrained Baseline / CQP / all-IDR / CPU upload only), and the zero-hardware-
//! verification caveat for this crate as authored. [ADR-0002](../../adr/linux/0002-vaapi-h264-p-frame-gop.md)
//! extends this to real single-forward-reference P-frame GOP structures, capability-gated on
//! `VAConfigAttribEncMaxRefFrames` — see [`gop`](super::gop) for the ported `GopState` decision
//! state machine driving `frame_num`/reference-list construction below.

use std::collections::VecDeque;
use std::rc::Rc;

use crate::{EncodeError, VideoEncoder, VideoEncoderConfig, VideoInputPreference};
use mediaway_common::{
    Bytes, CodecKind, Packet, PixelFormat, StreamInfo, VideoFrame, VideoFrameStorage, VideoGeometry,
};

use cros_libva::{
    BufferType, Config, Context, Display, EncPictureParameter, EncSequenceParameter,
    EncSliceParameter, H264EncPicFields, H264EncSeqFields, Image, MappedCodedBuffer, Picture,
    PictureH264, Surface, UsageHint, VA_ATTRIB_NOT_SUPPORTED, VA_FOURCC_NV12, VA_INVALID_ID,
    VA_LSB_FIRST, VA_PICTURE_H264_SHORT_TERM_REFERENCE, VA_RC_CQP, VA_RT_FORMAT_YUV420,
    VAConfigAttrib, VAConfigAttribType, VAEntrypoint, VAImageFormat,
};

use super::codec::video_profile;
use super::gop::{DpbSlot, FrameDecision, FrameRequest, GopState, LOG2_MAX_FRAME_NUM_MINUS4};

/// Number of driver surfaces kept in rotation — aliases [`gop::WORKSPACE_DPB_CAP`](super::gop)
/// so the physical surface pool and the logical DPB ring [`GopState`] tracks always stay the
/// same size by construction (ADR-0002's porting table: "happens to equal this crate's
/// pre-existing `SURFACE_POOL_SIZE`... so the physical surface pool needs no size change, only a
/// new selection strategy").
const SURFACE_POOL_SIZE: usize = super::gop::WORKSPACE_DPB_CAP;

/// Fixed quantization parameter used under `VA_RC_CQP` (range 0..=51). Mid-range value;
/// this stage does not expose a quality/bitrate knob — see ADR-0001 § Scope.
const FIXED_QP: u8 = 26;

/// H.264 Level 3.0 (`level_idc` value, i.e. level * 10) — generous for small CQP test frames.
const LEVEL_IDC: u8 = 30;

/// `log2_max_frame_num_minus4` used when GOP encode is disabled — this crate's pre-ADR-0002
/// value (`frame_num` always `0`; ample range for a field that never advances).
const IDR_ONLY_LOG2_MAX_FRAME_NUM_MINUS4: u8 = 4;

/// VA-API H.264 encode session (Constrained Baseline, CQP, CPU NV12 upload).
///
/// `gop_size <= 1` (the default) or a driver that does not report
/// `VAConfigAttribEncMaxRefFrames` support (see [`probe_supports_p_frames`]) both fall back to
/// all-IDR encode, byte-identical to this crate's pre-ADR-0002 output — see
/// [`VideoEncoderConfig::gop_size`]'s own documented fallback contract.
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
    /// GOP decision state (ADR-0002) — replaces the pre-ADR-0002 `next_surface: usize`
    /// round-robin cursor; `decision.setup_slot` is now the sole slot-selection strategy.
    gop: GopState,
    /// The GOP size this session actually honors: `1` when GOP mode is disabled (default
    /// `config.gop_size <= 1`, or [`supports_p_frames`](Self::supports_p_frames) is `false`),
    /// `config.gop_size` otherwise. Not part of ADR-0002's `VaapiVideoEncoder` sketch verbatim —
    /// added because [`build_seq_params`]/[`build_pic_params`] need to know whether GOP mode is
    /// active for the *whole session* (SPS `intra_period`/`log2_max_frame_num_minus4`,
    /// `reference_pic_flag`), which a single [`FrameDecision`] alone cannot tell them (an IDR
    /// decision looks the same whether it is the one-and-only frame of an all-IDR session or
    /// just the periodic IDR of an active GOP). See this ADR's implementation addendum.
    effective_gop_size: u32,
    /// Whether the driver reported `VAConfigAttribEncMaxRefFrames` support at `open_cpu` time
    /// (ADR-0002's capability gate). The gating decision itself is already baked into
    /// [`Self::effective_gop_size`] by the time this struct exists — this field exists so
    /// `video_tests.rs`'s hardware-gated tests can distinguish "driver lacks the capability"
    /// from other skip reasons.
    #[allow(
        dead_code,
        reason = "read only by video_tests.rs's hardware-gated tests; a plain `cargo check` \
                  without --tests never sees that call site (mirrors mediaway-decoder's \
                  dpb.rs::Dpb::capacity precedent)"
    )]
    supports_p_frames: bool,
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

        // ADR-0002 capability gate: probe before trusting `config.gop_size > 1` — this backend's
        // first "probe first, never assume" capability query.
        let supports_p_frames = probe_supports_p_frames(&display, profile);
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
            gop: GopState::new(effective_gop_size),
            effective_gop_size,
            supports_p_frames,
            pending: VecDeque::new(),
            flushed: false,
        })
    }

    /// Encode one frame from `surface` (already holding the uploaded NV12 picture) per
    /// `decision` (ADR-0002's `GopState::decide` output), reading `reference`'s
    /// `(VASurfaceID, DpbSlot)` as `RefPicList0[0]`/`ReferenceFrames[0]` when this is a P frame.
    ///
    /// Returns the surface to give back to the pool slot, when one is recoverable, alongside
    /// the encode result. `Picture::begin`/`render`/`end` take their receiver by value and
    /// only give it back embedded in the next typestate on success, so a failure at those
    /// steps loses the surface for good (the `VaError` they return does not carry it); this
    /// is reported as `None` rather than fabricating or panicking to produce a replacement —
    /// see [`Self::push_frame`], which tolerates a pool slot staying empty. Under GOP mode
    /// (ADR-0002), a lost setup-slot surface here can later surface as a lost *reference*
    /// slot too — guarded against in [`Self::push_frame`] before this method is even called.
    fn encode_one(
        &self,
        surface: Surface<()>,
        frame: &VideoFrame,
        decision: &FrameDecision,
        reference: Option<(cros_libva::VASurfaceID, DpbSlot)>,
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
        let gop_active = self.effective_gop_size > 1;

        // EncSequenceParameter is sent only on IDR frames once GOP mode is active — sending a
        // fresh SPS/PPS ahead of every P-frame would defeat ADR-0002's own bandwidth-efficiency
        // motivation. `gop_size <= 1` still sends it every frame (every frame is IDR there),
        // matching this crate's pre-ADR-0002 behavior byte-for-byte.
        let seq_buf = if decision.is_idr {
            let seq_params = build_seq_params(
                self.mb_width,
                self.mb_height,
                self.bits_per_second,
                self.effective_gop_size,
            );
            match self.context.create_buffer(BufferType::EncSequenceParameter(
                EncSequenceParameter::H264(seq_params),
            )) {
                Ok(buf) => Some(buf),
                Err(_) => return (Some(surface), Err(EncodeError::Backend)),
            }
        } else {
            None
        };

        let pic_params =
            build_pic_params(surface_id, coded_buf.id(), decision, reference, gop_active);
        let slice_params = build_slice_params(num_macroblocks, decision, reference);

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
        if let Some(seq_buf) = seq_buf {
            picture.add_buffer(seq_buf);
        }
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
            is_keyframe: decision.is_idr,
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

        // Step 2 (ADR-0002 § Reference-list construction): `decide`'s own side effects (DPB
        // bookkeeping, counter advancement) happen unconditionally and are never retried —
        // `GopState::decide` is not idempotent.
        let decision = self.gop.decide(FrameRequest::Auto);

        // Step 3 — the lost-reference-surface guard (ADR-0002 § Real gap found), with no
        // Vulkan-side precedent: a missing reference slot must fail this call hard, before
        // `decision.setup_slot`'s surface is even touched, since `GopState`'s bookkeeping above
        // already advanced and cannot be un-done without desyncing it from the physical session.
        let reference = match decision.reference {
            Some((ref_slot, ref_dpb_slot)) => {
                let ref_surface = self.surfaces[ref_slot]
                    .as_ref()
                    .ok_or(EncodeError::Backend)?;
                Some((ref_surface.id(), ref_dpb_slot))
            }
            None => None,
        };

        // Step 4: unchanged shape, new index source (`decision.setup_slot` replaces the old
        // `next_surface` round-robin cursor).
        let slot = decision.setup_slot;
        let surface = self.surfaces[slot].take().ok_or(EncodeError::Backend)?;

        // Step 5: fresh, this-frame's-own pixel content, whether IDR or P — the reference
        // slot's surface (a different index) is never uploaded into by this call.
        let surface = match upload_cpu_nv12(&surface, data, self.width, self.height) {
            Ok(()) => surface,
            Err(e) => {
                self.surfaces[slot] = Some(surface);
                return Err(e);
            }
        };

        // Step 6.
        let (returned_surface, result) = self.encode_one(surface, frame, &decision, reference);
        // Step 7: the surface at `ref_slot` (if any) is never taken or mutated above — VA-API's
        // own encode convention is that `CurrPic`'s surface implicitly holds the reconstructed
        // picture after `vaEndPicture`, usable as a later frame's reference with no separate DPB
        // image. May leave `slot` `None` when the surface was unrecoverably consumed by a failed
        // `Picture` step (see `encode_one`'s doc comment) — the next `push_frame` call's
        // `take().ok_or(EncodeError::Backend)` above already handles a `None` slot honestly.
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

/// Probes `VAConfigAttribEncMaxRefFrames` (ADR-0002 § VA-API-specific plumbing) before this
/// backend trusts `config.gop_size > 1` — the first "probe first, never assume" capability gate
/// this VA-API encoder backend needs, mirroring `mediaway-encoder::vulkan`'s
/// `Capabilities::supports_p_frames` precedent. A driver that does not support the attribute at
/// all reports `VA_ATTRIB_NOT_SUPPORTED`; this gate also requires a non-zero value — the
/// attribute's internal packed-value bit layout (low bits = max P/forward references, per
/// general `va_enc_h264.h` convention) was not independently confirmed against a real driver
/// this session, but this binary supported/unsupported check does not depend on that layout.
fn probe_supports_p_frames(display: &Display, profile: cros_libva::VAProfile::Type) -> bool {
    let mut attribs = [VAConfigAttrib {
        type_: VAConfigAttribType::VAConfigAttribEncMaxRefFrames,
        value: 0,
    }];
    let Ok(()) =
        display.get_config_attributes(profile, VAEntrypoint::VAEntrypointEncSlice, &mut attribs)
    else {
        return false;
    };
    attribs[0].value != VA_ATTRIB_NOT_SUPPORTED && attribs[0].value != 0
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

/// Sequence parameter set — sent only on IDR frames once GOP mode is active (see
/// `encode_one`). `pic_order_cnt_type = 2` so no explicit POC bookkeeping is needed (unchanged
/// by ADR-0002 — see the ADR's § Cross-check for the resulting, deliberately-unresolved
/// interop gap against this workspace's own VA-API decoder). `effective_gop_size <= 1`
/// reproduces this crate's pre-ADR-0002 SPS values exactly (`intra_period`/`intra_idr_period`/
/// `ip_period` = `1`/`1`/`0`, `log2_max_frame_num_minus4` = `4`).
fn build_seq_params(
    mb_width: u16,
    mb_height: u16,
    bits_per_second: u32,
    effective_gop_size: u32,
) -> cros_libva::EncSequenceParameterBufferH264 {
    let gop_active = effective_gop_size > 1;
    let log2_max_frame_num_minus4 = if gop_active {
        LOG2_MAX_FRAME_NUM_MINUS4
    } else {
        IDR_ONLY_LOG2_MAX_FRAME_NUM_MINUS4
    };
    // intra_idr_period = 1: every intra-period boundary is an IDR (this crate never emits a
    // non-IDR I picture) — VA-API's own convention (`intra_idr_period` counts *intra periods*
    // between IDRs, not frames).
    let (intra_period, intra_idr_period, ip_period) = if gop_active {
        (effective_gop_size, 1, 1)
    } else {
        (1, 1, 0)
    };

    let seq_fields = H264EncSeqFields::new(
        1, // chroma_format_idc: 4:2:0
        1, // frame_mbs_only_flag: progressive only
        0, // mb_adaptive_frame_field_flag
        0, // seq_scaling_matrix_present_flag
        1, // direct_8x8_inference_flag
        u32::from(log2_max_frame_num_minus4),
        2, // pic_order_cnt_type = 2: POC derived from frame_num, no explicit fields needed
        0, // log2_max_pic_order_cnt_lsb_minus4 (unused, pic_order_cnt_type != 0)
        0, // delta_pic_order_always_zero_flag (unused, pic_order_cnt_type != 1)
    );

    cros_libva::EncSequenceParameterBufferH264::new(
        0, // seq_parameter_set_id
        LEVEL_IDC,
        intra_period,
        intra_idr_period,
        ip_period,
        bits_per_second,
        1, // max_num_ref_frames: single-forward-reference design, at most one active reference
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

/// Picture parameter set. `decision.is_idr` drives `idr_pic_flag`/`primary_pic_type`;
/// `reference_pic_flag` is gated on `gop_active` (not `decision.is_idr` alone) so the default
/// (`gop_size <= 1`) path stays byte-identical to this crate's pre-ADR-0002 output
/// (`reference_pic_flag: 0` unconditionally there) while every frame — IDR and P alike —
/// within a truly active GOP is marked as a candidate future reference, matching
/// `GopState::decide`'s own bookkeeping (every setup slot is recorded regardless of `is_idr`).
/// `reference`, when `Some`, becomes `ReferenceFrames[0]`; the other 15 entries stay
/// [`invalid_picture_h264`].
fn build_pic_params(
    surface_id: cros_libva::VASurfaceID,
    coded_buf_id: cros_libva::VABufferID,
    decision: &FrameDecision,
    reference: Option<(cros_libva::VASurfaceID, DpbSlot)>,
    gop_active: bool,
) -> cros_libva::EncPictureParameterBufferH264 {
    // frame_idx/TopFieldOrderCnt/BottomFieldOrderCnt tracking this picture's own frame_num/poc
    // (not just the referenced-picture case ADR-0002 spells out) mirrors FFmpeg's
    // `vaapi_encode_h264.c` convention (`CurrPic.frame_idx = frame_num`) and is a no-op for the
    // default `gop_size <= 1` path, where both are always `0` — see this ADR's implementation
    // addendum.
    let curr_pic = PictureH264::new(
        surface_id,
        decision.frame_num,
        0,
        decision.poc,
        decision.poc,
    );
    let mut reference_frames: [PictureH264; 16] = std::array::from_fn(|_| invalid_picture_h264());
    if let Some((ref_surface_id, ref_slot)) = reference {
        reference_frames[0] = reference_picture_h264(ref_surface_id, ref_slot);
    }

    let pic_fields = H264EncPicFields::new(
        u32::from(decision.is_idr), // idr_pic_flag
        u32::from(gop_active),      // reference_pic_flag: see doc comment above
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

    #[allow(
        clippy::cast_possible_truncation,
        reason = "GopState::decide bounds frame_num < 1 << (LOG2_MAX_FRAME_NUM_MINUS4 + 4) == \
                  65536, always representable in u16 (ADR-0002 § VA-API-specific plumbing)"
    )]
    let frame_num = decision.frame_num as u16;

    cros_libva::EncPictureParameterBufferH264::new(
        curr_pic,
        reference_frames,
        coded_buf_id,
        0, // pic_parameter_set_id
        0, // seq_parameter_set_id
        0, // last_picture: no end-of-sequence/stream signaling this stage
        frame_num,
        FIXED_QP,
        0, // num_ref_idx_l0_active_minus1: exactly one active L0 reference when present
        0, // num_ref_idx_l1_active_minus1 (unused: no B slices)
        0, // chroma_qp_index_offset
        0, // second_chroma_qp_index_offset
        &pic_fields,
    )
}

/// Single-slice-per-frame slice parameters covering the whole picture. `decision.is_idr`
/// selects `slice_type` (I vs P); `reference`, when `Some`, becomes `RefPicList0[0]` — the sole
/// entry this crate's single-forward-reference design ever populates.
fn build_slice_params(
    num_macroblocks: u32,
    decision: &FrameDecision,
    reference: Option<(cros_libva::VASurfaceID, DpbSlot)>,
) -> cros_libva::EncSliceParameterBufferH264 {
    let mut ref_pic_list_0: [PictureH264; 32] = std::array::from_fn(|_| invalid_picture_h264());
    let ref_pic_list_1: [PictureH264; 32] = std::array::from_fn(|_| invalid_picture_h264());
    if let Some((ref_surface_id, ref_slot)) = reference {
        ref_pic_list_0[0] = reference_picture_h264(ref_surface_id, ref_slot);
    }
    // VA-API/H.264's own numeric convention: P = 0, B = 1, I = 2.
    let slice_type: u8 = if decision.is_idr { 2 } else { 0 };

    cros_libva::EncSliceParameterBufferH264::new(
        0, // macroblock_address: whole frame in one slice
        num_macroblocks,
        VA_INVALID_ID, // macroblock_info: no per-macroblock override
        slice_type,
        0, // pic_parameter_set_id
        decision.idr_pic_id,
        0,         // pic_order_cnt_lsb (unused, pic_order_cnt_type == 2)
        0,         // delta_pic_order_cnt_bottom (unused, pic_order_cnt_type == 2)
        [0i32; 2], // delta_pic_order_cnt (unused, pic_order_cnt_type == 2)
        0,         // direct_spatial_mv_pred_flag (unused: no B slices)
        0,         // num_ref_idx_active_override_flag: use the picture-level default (1)
        0,         // num_ref_idx_l0_active_minus1 (unused unless override_flag is set)
        0,         // num_ref_idx_l1_active_minus1 (unused: no B slices)
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

/// A `VAPictureH264` reference-list/`ReferenceFrames` entry describing one already-encoded
/// picture: `frame_idx` = the referenced picture's own `frame_num` (inferred from `FFmpeg`'s
/// `vaapi_encode_h264.c` convention — not independently confirmed against a real driver this
/// session, ADR-0002 § Open questions item 2), `flags` = short-term reference,
/// `TopFieldOrderCnt`/`BottomFieldOrderCnt` = the referenced picture's `poc` (progressive only,
/// no field coding).
fn reference_picture_h264(surface_id: cros_libva::VASurfaceID, slot: DpbSlot) -> PictureH264 {
    PictureH264::new(
        surface_id,
        slot.frame_num,
        VA_PICTURE_H264_SHORT_TERM_REFERENCE,
        slot.poc,
        slot.poc,
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
    // ADR-0002: `0` is rejected at `open()` time (runtime `EncodeError`, not silently treated as
    // "unlimited GOP") per `VideoEncoderConfig::gop_size`'s own documented contract.
    if config.gop_size == 0 {
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
