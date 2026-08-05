//! Public [`crate::VideoEncoder`] entry point: a reusable,
//! multi-frame `VulkanVideoEncoder` (H.264 or HEVC) built on the Stage 1
//! session machinery in
//! [`crate::vulkan::session`]/[`crate::vulkan::session_encode`]/[`crate::vulkan::session_command`]/
//! [`crate::vulkan::session_command_hevc`].
//!
//! Unlike [`crate::vulkan::session_encode::encode_synthetic_intra_frame`] (a one-shot
//! diagnostic that builds and tears down a whole session for a single
//! synthetic frame), this type keeps the instance/device/video session/
//! session parameters/images/buffers/command pool/fence/query pool alive
//! across every [`VideoEncoder::push_frame`] call — only the per-frame
//! upload/record/submit/readback repeats, mirroring
//! `mediaway-encoder-windows`'s `D3d12VideoEncoder` session shape. CPU-upload
//! NV12 input only (this crate's Stage 3 Zero-Copy external-memory import is
//! still deferred). Every pushed frame is an independent key frame by
//! default (`gop_size == 1`); ADR-0002 adds capability-gated multi-frame GOP
//! with real P-frame DPB reference reuse for H.264 and HEVC, plus (its AV1
//! follow-up) real `LAST_FRAME` single-forward-reference cycling for AV1 too
//! — **implemented but unverifiable on this crate's reference hardware**,
//! since AV1's underlying per-frame encode is already hardware-verified
//! invalid there (driver-maturity limitation, `adr/0001`'s AV1 addendum).
//! Never B-frames, any codec — see ADR-0002. CBR rate control stays
//! H.264-only.

#![allow(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    reason = "idr_pic_id is a wrapping u16 counter — mirrors session_encode.rs's/\
              session_command.rs's crate-wide allow for small driver-facing counts"
)]

use std::collections::VecDeque;

use crate::{EncodeError, VideoEncoder, VideoEncoderConfig, VideoInputPreference};
use mediaway_common::{
    Bytes, CodecKind, Packet, PixelFormat, StreamInfo, VideoFrame, VideoFrameStorage, VideoGeometry,
};
use vulkanalia::vk;
use vulkanalia::vk::{DeviceV1_0, HasBuilder, InstanceV1_0};

use crate::vulkan::av1_gop::{self, FrameRequest as Av1FrameRequest, GopState as Av1GopState};
use crate::vulkan::av1_params::{self, Av1SeqGopParams, InterFramePrediction};
use crate::vulkan::h264_gop::{FrameRequest, GopState, WORKSPACE_DPB_CAP};
use crate::vulkan::h264_params::{self, McAlignedExtent, SpsGopParams};
use crate::vulkan::hevc_gop::{FrameRequest as HevcFrameRequest, GopState as HevcGopState};
use crate::vulkan::hevc_params::{self, CtuAlignedExtent, SpsGopParams as HevcSpsGopParams};
use crate::vulkan::session::{
    DeviceGuard, EncodeDevice, EncodeProfile, InstanceGuard, SessionDpbConfig, SessionResources,
    VulkanEncodeSessionError, create_instance, create_logical_device, find_av1_encode_device,
    find_h264_encode_device, find_hevc_encode_device, query_capabilities, query_video_format,
};
use crate::vulkan::session_command::{
    DpbRecordParams, RateControlParams, RecordParams, record_and_submit,
};
use crate::vulkan::session_command_av1::{
    DpbRecordParamsAv1, RecordParamsAv1, record_and_submit_av1,
};
use crate::vulkan::session_command_hevc::{
    DpbRecordParamsHevc, RecordParamsHevc, record_and_submit_hevc,
};
use crate::vulkan::session_encode::{
    allocate_command_buffer, create_command_pool, create_encode_feedback_query_pool, create_fence,
    create_images_and_buffers, create_session_parameters, create_session_parameters_av1,
    create_session_parameters_hevc, create_video_session, get_encoded_headers,
    get_encoded_headers_av1, get_encoded_headers_hevc, nv12_byte_size, upload_to_host_memory,
};

/// Fixed intra `constant_qp` (all-intra CQP) — this crate's only mode when
/// rate control is disabled (`DISABLED` fixed-QP, mirrors
/// `mediaway-encoder-windows`'s D3D12 backend's `FIXED_QP`) or for HEVC/AV1
/// (unconditionally, ADR-0002 scopes CBR to H.264 this pass).
const FIXED_QP: i32 = 26;

/// A real, reusable, hardware-backed `VK_KHR_video_encode_queue` H.264 or HEVC session.
///
/// Owns its own Vulkan instance and logical device — this backend never
/// imports a caller-supplied device (see the module doc).
pub struct VulkanVideoEncoder {
    codec: CodecKind,
    // Field order is load-bearing: Rust drops struct fields top-to-bottom,
    // and Vulkan requires every `VkDevice` to be destroyed before the
    // `VkInstance` it was created from (`device_guard` must drop — and
    // therefore be declared — before `_instance_guard`). `resources` is torn
    // down explicitly in this type's own `Drop::drop` before any of these
    // fields' auto-drop glue runs at all, so its position doesn't matter for
    // ordering, only that `device_guard` is still valid when that explicit
    // teardown runs (it is — custom `Drop::drop` always completes before the
    // compiler-generated field drops start).
    device_guard: DeviceGuard,
    _instance_guard: InstanceGuard,
    _entry: vulkanalia::Entry,
    queue: vk::Queue,
    queue_family_index: u32,

    resources: SessionResources,
    command_buffer: vk::CommandBuffer,
    coded_extent: vk::Extent2D,
    dst_size: vk::DeviceSize,
    /// Fixed per-session Annex-B SPS+PPS, cloned into every packet — every
    /// pushed frame is an independent key frame, so headers must accompany
    /// each one (mirrors `mediaway-encoder-windows`'s D3D12 backend).
    header_bytes: Vec<u8>,

    info: StreamInfo,
    pending: VecDeque<Packet>,
    flushed: bool,
    frame_counter: u32,

    // --- ADR-0002: GOP / rate control (H.264 + HEVC GOP, H.264-only CBR) ---
    /// Owned unconditionally (even for a non-H.264 session, where it stays
    /// at its `gop_size == 1` default and is never read for HEVC/AV1
    /// sessions) — cheaper and simpler than an `Option<GopState>` whose
    /// `None`/`Some` split would just mirror `codec == H264`, per this
    /// crate's own "no `Option` wrapping an invariant the type system could
    /// express directly" preference.
    gop_state: GopState,
    /// HEVC sibling of `gop_state` — same "owned unconditionally, idle
    /// unless `codec == Hevc`" reasoning; a separate field (not shared with
    /// `gop_state`) since [`GopState`]/[`HevcGopState`] are distinct types
    /// (see `hevc_gop.rs`'s module doc for why they aren't unified).
    hevc_gop_state: HevcGopState,
    /// AV1 sibling of `gop_state`/`hevc_gop_state` (ADR-0002's AV1
    /// follow-up) — same "owned unconditionally, idle unless `codec == Av1`"
    /// reasoning; a separate type from both since AV1's `order_hint`-keyed
    /// reference model has no `frame_num`/`PicOrderCnt` equivalent (see
    /// `av1_gop.rs`'s module doc). Real, capability-gated GOP wiring built on
    /// top of an already-known-broken AV1 base encode — see that module doc
    /// for the honest "implemented but unverifiable" status.
    av1_gop_state: Av1GopState,
    /// `true` for an H.264, HEVC, or AV1 session where
    /// `capabilities.supports_p_frames` and `config.gop_size > 1` were both
    /// true at `open()` time — gates every GOP-specific FFI shape (DPB slot
    /// info chaining, multi-layer DPB image, non-`DISABLED`-shaped session/SPS
    /// params) so the default path reproduces the original all-key-frame call
    /// shape untouched. AV1's own GOP path is real and capability-gated the
    /// same way, but unverifiable on this crate's reference hardware — see
    /// `av1_gop.rs`'s module doc.
    gop_enabled: bool,
    /// `resources.dpb_image`/`dpb_image_view`'s actual array layer count —
    /// `1` unless `gop_enabled`.
    dpb_layer_count: u32,
    /// Whether `resources.dpb_image` still needs its one-time
    /// `UNDEFINED -> VIDEO_ENCODE_DPB_KHR` layout transition (see
    /// `session_command::record_pre_encode_barriers`'s doc). Set after the
    /// first successful `push_frame` call.
    dpb_transitioned: bool,
    /// `Some` (H.264 only, capability- and config-gated — ADR-0002 scopes
    /// CBR to H.264 this pass) replaces every pushed frame's
    /// `RATE_CONTROL_MODE_DISABLED` with real `CBR`.
    rate_control_params: Option<RateControlParams>,
}

// SAFETY: every field is an owned Vulkan handle/`ash` wrapper (thread-safe
// per the Vulkan spec's external-synchronization rules — this type provides
// that synchronization itself via `&mut self`) or plain owned data.
unsafe impl Send for VulkanVideoEncoder {}

impl VulkanVideoEncoder {
    /// Opens a real H.264 or HEVC Vulkan Video encode session for `config`.
    ///
    /// # Errors
    ///
    /// - [`EncodeError::Unsupported`] — `config.codec` is neither
    ///   [`CodecKind::H264`] nor [`CodecKind::Hevc`], `config.pixel_format`
    ///   is not [`PixelFormat::Nv12`], `config.input` is not
    ///   [`VideoInputPreference::CpuUploadOk`], no Vulkan loader/device on
    ///   this host advertises an encode queue family for the requested
    ///   codec, or the driver reports no usable video-encode image format.
    /// - [`EncodeError::InvalidInput`] — zero/invalid dimensions, zero
    ///   `time_base` denominator, `config.gop_size == 0` (ADR-0002 — `0` is
    ///   rejected, never treated as "unlimited GOP"), or
    ///   `config.width`/`height` falls outside this driver's reported
    ///   coded-extent bounds/alignment.
    /// - [`EncodeError::Backend`] — a Vulkan object-creation call failed.
    ///
    /// `config.gop_size > 1` (multi-frame GOP with P-frame prediction) is a
    /// capability-gated request for H.264 or HEVC (ADR-0002); `config.rate_control.is_some()`
    /// (CBR) stays a capability-gated, H.264-only request this pass. A
    /// driver/profile that cannot honor either falls back to today's
    /// IDR-only, fixed-QP `DISABLED` behavior with no error
    /// (`Capabilities::supports_p_frames`/`supports_cbr`). AV1 never honors
    /// GOP or CBR, unconditionally (out of scope — see ADR-0002).
    #[allow(
        clippy::too_many_lines,
        reason = "linear session-construction sequence (instance -> device -> capabilities -> \
                  session -> parameters -> images/buffers -> command pool/fence/query pool), \
                  mirrors mediaway-encoder-windows's D3d12VideoEncoder::open"
    )]
    pub fn open(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        validate_common(config)?;
        let is_hevc = config.codec == CodecKind::Hevc;
        let is_av1 = config.codec == CodecKind::Av1;

        let (entry, instance_guard) = create_instance().map_err(map_err)?;
        let InstanceGuard { instance } = &instance_guard;
        let (physical_device, queue_family_index) = if is_av1 {
            find_av1_encode_device(instance)
        } else if is_hevc {
            find_hevc_encode_device(instance)
        } else {
            find_h264_encode_device(instance)
        }
        .map_err(map_err)?;

        let mut profile = if is_av1 {
            EncodeProfile::new_av1()
        } else if is_hevc {
            EncodeProfile::new_hevc()
        } else {
            EncodeProfile::new_h264()
        };
        let capabilities =
            query_capabilities(instance, physical_device, &mut profile).map_err(map_err)?;
        capabilities
            .validate_requested_extent(config.width, config.height)
            .map_err(map_err)?;
        let coded_extent = vk::Extent2D {
            width: config.width,
            height: config.height,
        };

        // ADR-0002 (+ its AV1 follow-up): GOP falls back to the original
        // all-key-frame shape whenever the caller left `gop_size` at its
        // default, or the driver can't honor the request
        // (`capabilities.supports_p_frames`, no error, a documented
        // degradation per `caveats-and-clarity.md`). AV1's own GOP request is
        // real and capability-gated the same way as H.264/HEVC — but
        // unverifiable end to end on this crate's reference hardware, since
        // AV1's base per-frame encode is already known-broken there (see
        // `adr/0001`'s AV1 addendum and `av1_gop.rs`'s module doc). CBR stays
        // H.264-only this pass — see `rate_control_params` below.
        let is_h264 = !is_hevc && !is_av1;
        let supports_gop_for_codec =
            (is_h264 || is_hevc || is_av1) && capabilities.supports_p_frames;
        let effective_gop_size = if supports_gop_for_codec {
            config.gop_size
        } else {
            1
        };
        let gop_enabled = effective_gop_size > 1;
        let dpb_layer_count = if gop_enabled {
            capabilities
                .max_dpb_slots
                .min(u32::try_from(WORKSPACE_DPB_CAP).unwrap_or(4))
                .max(2)
        } else {
            1
        };
        let dpb_config = if gop_enabled {
            SessionDpbConfig {
                max_dpb_slots: dpb_layer_count,
                // This crate only ever requests one active L0 reference
                // (single forward reference, never multi-reference search —
                // see `h264_gop.rs`'s module doc).
                max_active_reference_pictures: 1,
            }
        } else {
            SessionDpbConfig::IDR_ONLY
        };
        let sps_gop = if gop_enabled {
            SpsGopParams {
                log2_max_frame_num_minus4: crate::vulkan::h264_gop::LOG2_MAX_FRAME_NUM_MINUS4,
                max_num_ref_frames: 1,
            }
        } else {
            SpsGopParams::IDR_ONLY
        };
        let hevc_sps_gop = if gop_enabled {
            HevcSpsGopParams {
                log2_max_pic_order_cnt_lsb_minus4:
                    crate::vulkan::hevc_gop::LOG2_MAX_PIC_ORDER_CNT_LSB_MINUS4,
            }
        } else {
            HevcSpsGopParams::IDR_ONLY
        };
        let av1_sps_gop = if gop_enabled {
            Av1SeqGopParams {
                order_hint_bits_minus_1: av1_gop::ORDER_HINT_BITS_MINUS_1_GOP,
            }
        } else {
            Av1SeqGopParams::IDR_ONLY
        };
        // ADR-0002 scopes CBR to H.264 only this pass — HEVC always stays on
        // today's fixed-QP `DISABLED` path (`session_command_hevc.rs`'s
        // `record_video_coding_hevc` never reads `rate_control_params`).
        let rate_control_params = if is_h264 && capabilities.supports_cbr {
            config.rate_control.map(|rc| {
                let vbv_ms = rc.vbv_buffer_size_bytes.map_or(0, |bytes| {
                    // bits = bytes * 8; ms = bits * 1000 / bitrate_bps. `0`
                    // (unset `vbv_buffer_size_bytes`, or a degenerate
                    // `target_bitrate_bps == 0`) lets the driver pick its own
                    // default VBV size instead of this crate guessing one.
                    let bits = u64::from(bytes) * 8;
                    if rc.target_bitrate_bps == 0 {
                        0
                    } else {
                        u32::try_from(bits.saturating_mul(1000) / u64::from(rc.target_bitrate_bps))
                            .unwrap_or(u32::MAX)
                    }
                });
                RateControlParams {
                    average_bitrate_bps: u64::from(rc.target_bitrate_bps),
                    max_bitrate_bps: u64::from(rc.target_bitrate_bps),
                    frame_rate_numerator: config.time_base.den,
                    frame_rate_denominator: u32::try_from(config.time_base.num).unwrap_or(1),
                    virtual_buffer_size_in_ms: vbv_ms,
                }
            })
        } else {
            None
        };

        let input_format = query_video_format(
            instance,
            physical_device,
            &mut profile,
            vk::ImageUsageFlags::VIDEO_ENCODE_SRC_KHR,
        )
        .map_err(map_err)?;
        let dpb_format = query_video_format(
            instance,
            physical_device,
            &mut profile,
            vk::ImageUsageFlags::VIDEO_ENCODE_DPB_KHR,
        )
        .map_err(map_err)?;

        let device_guard = create_logical_device(instance, physical_device, queue_family_index)
            .map_err(map_err)?;
        let device = &device_guard.device;
        // SAFETY: `queue_family_index`/index `0` were the exact queue this
        // `device` was created with above.
        let queue = unsafe { device.get_device_queue(queue_family_index, 0) };
        let encode_device = EncodeDevice {
            device,
            queue,
            queue_family_index,
        };
        // SAFETY: `physical_device` came from `find_h264_encode_device`/
        // `find_hevc_encode_device` on this same `instance`.
        let memory_properties =
            unsafe { instance.get_physical_device_memory_properties(physical_device) };

        let mut resources = SessionResources::default();

        let (session, session_memories) = create_video_session(
            &encode_device,
            &memory_properties,
            &mut profile,
            &capabilities,
            coded_extent,
            input_format,
            dpb_format,
            dpb_config,
        )
        .map_err(map_err)?;
        resources.session = session;
        resources.session_memories = session_memories;

        let header_bytes = if is_av1 {
            let color_config = av1_params::build_color_config();
            let timing_info = av1_params::build_timing_info();
            let sequence_header = av1_params::build_sequence_header(
                coded_extent.width,
                coded_extent.height,
                &color_config,
                &timing_info,
                av1_sps_gop,
            );
            let operating_point = av1_params::build_operating_point();
            resources.session_parameters = create_session_parameters_av1(
                &encode_device,
                session,
                &sequence_header,
                std::slice::from_ref(&operating_point),
            )
            .map_err(map_err)?;
            let (av1_header_bytes, _has_overrides) =
                get_encoded_headers_av1(&encode_device, resources.session_parameters)
                    .map_err(map_err)?;
            av1_header_bytes
        } else if is_hevc {
            let extent = CtuAlignedExtent::from_pixels(coded_extent.width, coded_extent.height)
                .ok_or(EncodeError::InvalidInput)?;
            let ptl = hevc_params::profile_tier_level_main();
            // ADR-0002: GOP mode needs a DPB deep enough for one active
            // reference (`dec_pic_buf_mgr_single_ref`); Stage 1's IDR-only
            // shape (`dec_pic_buf_mgr_no_refs`) is unchanged otherwise.
            let dpb_mgr = if gop_enabled {
                hevc_params::dec_pic_buf_mgr_single_ref()
            } else {
                hevc_params::dec_pic_buf_mgr_no_refs()
            };
            let vps = hevc_params::build_vps(&ptl, &dpb_mgr);
            let sps = hevc_params::build_sps(extent, &ptl, &dpb_mgr, hevc_sps_gop);
            let pps = hevc_params::build_pps();
            resources.session_parameters =
                create_session_parameters_hevc(&encode_device, session, &vps, &sps, &pps)
                    .map_err(map_err)?;
            get_encoded_headers_hevc(&encode_device, resources.session_parameters)
                .map_err(map_err)?
        } else {
            let extent = McAlignedExtent::from_pixels(coded_extent.width, coded_extent.height)
                .ok_or(EncodeError::InvalidInput)?;
            let sps = h264_params::build_sps(extent, sps_gop);
            let pps = h264_params::build_pps();
            resources.session_parameters =
                create_session_parameters(&encode_device, session, &sps, &pps).map_err(map_err)?;
            get_encoded_headers(&encode_device, resources.session_parameters).map_err(map_err)?
        };

        let dst_size = create_images_and_buffers(
            &encode_device,
            &memory_properties,
            &mut profile,
            input_format,
            dpb_format,
            coded_extent,
            capabilities.min_bitstream_buffer_size_alignment,
            dpb_layer_count,
            &mut resources,
        )
        .map_err(map_err)?;

        resources.command_pool =
            create_command_pool(device, queue_family_index).map_err(map_err)?;
        let command_buffer =
            allocate_command_buffer(device, resources.command_pool).map_err(map_err)?;
        resources.fence = create_fence(device).map_err(map_err)?;
        resources.encode_feedback_query_pool =
            create_encode_feedback_query_pool(device, &mut profile).map_err(map_err)?;

        Ok(Self {
            codec: config.codec,
            _entry: entry,
            _instance_guard: instance_guard,
            device_guard,
            queue,
            queue_family_index,
            resources,
            command_buffer,
            coded_extent,
            dst_size,
            header_bytes,
            info: stream_info_from(config),
            pending: VecDeque::new(),
            flushed: false,
            frame_counter: 0,
            gop_state: GopState::new(effective_gop_size),
            hevc_gop_state: HevcGopState::new(effective_gop_size),
            av1_gop_state: Av1GopState::new(effective_gop_size),
            gop_enabled,
            dpb_layer_count,
            dpb_transitioned: false,
            rate_control_params,
        })
    }
}

impl VideoEncoder for VulkanVideoEncoder {
    fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    #[allow(
        clippy::too_many_lines,
        reason = "linear three-codec (H.264/HEVC/AV1) picture-info dispatch — each branch \
                  mirrors its own params module's builder call sequence; splitting further \
                  would just move the same per-codec lines into a same-file helper"
    )]
    fn push_frame(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        if self.flushed {
            return Err(EncodeError::Closed);
        }
        let VideoFrameStorage::Cpu { data } = &frame.storage else {
            return Err(EncodeError::Unsupported);
        };
        if frame.width != self.coded_extent.width || frame.height != self.coded_extent.height {
            return Err(EncodeError::InvalidInput);
        }
        let expected_len = nv12_byte_size(self.coded_extent.width, self.coded_extent.height);
        let expected_len = usize::try_from(expected_len).map_err(|_| EncodeError::InvalidInput)?;
        if data.len() < expected_len {
            return Err(EncodeError::InvalidInput);
        }

        let device = &self.device_guard.device;
        upload_to_host_memory(device, self.resources.staging_memory, &data[..expected_len])
            .map_err(map_err)?;

        let encode_device = EncodeDevice {
            device,
            queue: self.queue,
            queue_family_index: self.queue_family_index,
        };
        let (dst_bytes, is_keyframe) = if self.codec == CodecKind::Av1 {
            // ADR-0002's AV1 follow-up: `self.av1_gop_state.decide`
            // reproduces the original all-key-frame sequencing exactly when
            // `!self.gop_enabled` (`Av1GopState::new` was constructed with
            // `effective_gop_size == 1` in that case, so every call returns
            // `is_key: true, order_hint: 0, reference: None` — see that
            // type's doc) — mirrors the H.264/HEVC branches' identical
            // reasoning below. **Implemented but unverifiable**: AV1's
            // underlying per-frame encode is already hardware-verified
            // invalid on this crate's reference GPU regardless of GOP mode
            // (`adr/0001`'s AV1 addendum) — this branch exists so the
            // capability gate and FFI shape are real, not because its output
            // can currently be confirmed correct.
            let decision = self.av1_gop_state.decide(Av1FrameRequest::Auto);
            let optionals = av1_params::PictureInfoOptionals::new();
            let (picture_info, prediction_mode, rate_control_group, reference_name_slot_indices) =
                match decision.reference {
                    None => {
                        let info = av1_params::build_key_frame_picture_info(
                            self.coded_extent.width,
                            self.coded_extent.height,
                            &optionals,
                        );
                        (
                            info,
                            vk::VideoEncodeAV1PredictionModeKHR::VIDEO_ENCODE_AV1_PREDICTION_MODE_INTRA_ONLY,
                            vk::VideoEncodeAV1RateControlGroupKHR::VIDEO_ENCODE_AV1_RATE_CONTROL_GROUP_INTRA,
                            [-1i32; 7],
                        )
                    }
                    Some((ref_slot, ref_dpb_slot)) => {
                        let ref_slot_i8 = i8::try_from(ref_slot).unwrap_or(0);
                        let prediction = InterFramePrediction {
                            order_hint: decision.order_hint,
                            setup_slot: u8::try_from(decision.setup_slot).unwrap_or(0),
                            ref_slot: ref_slot_i8,
                            ref_order_hint: ref_dpb_slot.order_hint,
                        };
                        let info = av1_params::build_inter_frame_picture_info(
                            self.coded_extent.width,
                            self.coded_extent.height,
                            &prediction,
                            &optionals,
                        );
                        let mut ref_indices = [-1i32; 7];
                        ref_indices[0] = i32::from(ref_slot_i8);
                        (
                            info,
                            vk::VideoEncodeAV1PredictionModeKHR::VIDEO_ENCODE_AV1_PREDICTION_MODE_SINGLE_REFERENCE,
                            vk::VideoEncodeAV1RateControlGroupKHR::VIDEO_ENCODE_AV1_RATE_CONTROL_GROUP_PREDICTIVE,
                            ref_indices,
                        )
                    }
                };
            let mut av1_picture_info = vk::VideoEncodeAV1PictureInfoKHR::builder()
                .prediction_mode(prediction_mode)
                .rate_control_group(rate_control_group)
                .constant_q_index(u32::from(av1_params::FIXED_Q_INDEX))
                .std_picture_info(&picture_info)
                .reference_name_slot_indices(reference_name_slot_indices)
                .primary_reference_cdf_only(false)
                .generate_obu_extension_header(false)
                .build();
            self.frame_counter = self.frame_counter.wrapping_add(1);

            // Every GOP-specific FFI shape stays gated on `self.gop_enabled`
            // — mirrors the H.264/HEVC branches' identical
            // `DpbRecordParams*` construction below.
            let transition = !self.gop_enabled || !self.dpb_transitioned;
            let reference_extension_header = av1_params::build_extension_header();
            let setup_reference_info = self.gop_enabled.then(|| {
                av1_params::build_reference_info(
                    decision.order_hint,
                    decision.is_key,
                    &reference_extension_header,
                )
            });
            let reference = if self.gop_enabled {
                decision.reference.map(|(slot, dpb_slot)| {
                    let info = av1_params::build_reference_info(
                        dpb_slot.order_hint,
                        dpb_slot.is_key,
                        &reference_extension_header,
                    );
                    (i32::try_from(slot).unwrap_or(0), info)
                })
            } else {
                None
            };
            let dpb = DpbRecordParamsAv1 {
                layer_count: self.dpb_layer_count,
                transition,
                setup_slot: i32::try_from(decision.setup_slot).unwrap_or(0),
                setup_reference_info,
                reference,
            };

            let mut record_params = RecordParamsAv1 {
                command_buffer: self.command_buffer,
                coded_extent: self.coded_extent,
                dst_size: self.dst_size,
                picture_info_pnext: &mut av1_picture_info,
                dpb,
            };
            let bytes =
                record_and_submit_av1(&encode_device, &mut self.resources, &mut record_params)
                    .map_err(map_err)?;
            self.dpb_transitioned = true;
            (bytes, decision.is_key)
        } else if self.codec == CodecKind::Hevc {
            // ADR-0002: `self.hevc_gop_state.decide` reproduces Stage 1's
            // all-IDR sequencing exactly when `!self.gop_enabled`
            // (`HevcGopState::new` was constructed with `effective_gop_size
            // == 1` in that case, so every call returns `is_idr: true,
            // reference: None` — see that type's doc) — mirrors the H.264
            // branch's identical reasoning above.
            let decision = self.hevc_gop_state.decide(HevcFrameRequest::Auto);
            let reference_slot = decision
                .reference
                .map(|(slot, _)| u8::try_from(slot).unwrap_or(0));
            let structs =
                hevc_params::build_frame_structs(decision.poc, decision.is_idr, reference_slot);
            let mut picture_info = structs.picture_info;
            picture_info.pRefLists = &raw const structs.reference_lists;
            if let Some(short_term_ref_pic_set) = &structs.short_term_ref_pic_set {
                picture_info.pShortTermRefPicSet = &raw const *short_term_ref_pic_set;
            }
            let nalu_slice_entries = [vk::VideoEncodeH265NaluSliceSegmentInfoKHR::builder()
                .constant_qp(FIXED_QP)
                .std_slice_segment_header(&structs.slice_segment_header)
                .build()];
            let mut hevc_picture_info = vk::VideoEncodeH265PictureInfoKHR::builder()
                .nalu_slice_segment_entries(&nalu_slice_entries)
                .std_picture_info(&picture_info)
                .build();
            self.frame_counter = self.frame_counter.wrapping_add(1);

            // Every GOP-specific FFI shape stays gated on `self.gop_enabled`
            // — mirrors the H.264 branch's identical `DpbRecordParams`
            // construction below, `StdVideoEncodeH265*` in place of
            // `StdVideoEncodeH264*`.
            let transition = !self.gop_enabled || !self.dpb_transitioned;
            let setup_reference_info = self.gop_enabled.then_some(structs.setup_reference_info);
            let reference = if self.gop_enabled {
                decision.reference.map(|(slot, dpb_slot)| {
                    let info = hevc_params::build_reference_info(dpb_slot.poc, dpb_slot.is_idr);
                    (i32::try_from(slot).unwrap_or(0), info)
                })
            } else {
                None
            };
            let dpb = DpbRecordParamsHevc {
                layer_count: self.dpb_layer_count,
                transition,
                setup_slot: i32::try_from(decision.setup_slot).unwrap_or(0),
                setup_reference_info,
                reference,
            };

            let mut record_params = RecordParamsHevc {
                command_buffer: self.command_buffer,
                coded_extent: self.coded_extent,
                dst_size: self.dst_size,
                picture_info_pnext: &mut hevc_picture_info,
                dpb,
            };
            let bytes =
                record_and_submit_hevc(&encode_device, &mut self.resources, &mut record_params)
                    .map_err(map_err)?;
            self.dpb_transitioned = true;
            (bytes, decision.is_idr)
        } else {
            // ADR-0002: `self.gop_state.decide` reproduces Stage 1's
            // all-IDR sequencing exactly when `!self.gop_enabled`
            // (`GopState::new` was constructed with `effective_gop_size ==
            // 1` in that case, so every call returns `is_idr: true,
            // reference: None` — see that type's doc) — this crate always
            // routes H.264 through the same GOP-aware path regardless of
            // `gop_size`, rather than keeping a separate legacy branch.
            let decision = self.gop_state.decide(FrameRequest::Auto);
            let reference_slot = decision
                .reference
                .map(|(slot, _)| u8::try_from(slot).unwrap_or(0));
            let structs = h264_params::build_frame_structs(
                decision.frame_num,
                decision.poc,
                decision.idr_pic_id,
                decision.is_idr,
                reference_slot,
            );
            let mut picture_info = structs.picture_info;
            picture_info.pRefLists = &raw const structs.reference_lists;
            // VUID-VkVideoEncodeH264NaluSliceInfoKHR-constantQp: must be `0`
            // whenever rate control is not `DISABLED`.
            let constant_qp = if self.rate_control_params.is_some() {
                0
            } else {
                FIXED_QP
            };
            let nalu_slice_entries = [vk::VideoEncodeH264NaluSliceInfoKHR::builder()
                .constant_qp(constant_qp)
                .std_slice_header(&structs.slice_header)
                .build()];
            let mut h264_picture_info = vk::VideoEncodeH264PictureInfoKHR::builder()
                .nalu_slice_entries(&nalu_slice_entries)
                .std_picture_info(&picture_info)
                .generate_prefix_nalu(false)
                .build();
            self.frame_counter = self.frame_counter.wrapping_add(1);

            // Every GOP-specific FFI shape stays gated on `self.gop_enabled`
            // — the default (`gop_size == 1`) path builds the exact same
            // `DpbRecordParams` Stage 1's diagnostic uses
            // (`DpbRecordParams::idr_only()`), just re-derived here instead
            // of imported, since `layer_count`/`transition` still need this
            // session's own state.
            let transition = !self.gop_enabled || !self.dpb_transitioned;
            let setup_reference_info = self.gop_enabled.then_some(structs.setup_reference_info);
            let reference = if self.gop_enabled {
                decision.reference.map(|(slot, dpb_slot)| {
                    let info = h264_params::build_reference_info(
                        dpb_slot.frame_num,
                        dpb_slot.poc,
                        dpb_slot.is_idr,
                    );
                    (i32::try_from(slot).unwrap_or(0), info)
                })
            } else {
                None
            };
            let dpb = DpbRecordParams {
                layer_count: self.dpb_layer_count,
                transition,
                setup_slot: i32::try_from(decision.setup_slot).unwrap_or(0),
                setup_reference_info,
                reference,
            };

            let mut record_params = RecordParams {
                command_buffer: self.command_buffer,
                coded_extent: self.coded_extent,
                dst_size: self.dst_size,
                picture_info_pnext: &mut h264_picture_info,
                dpb,
                rate_control: self.rate_control_params,
            };
            let bytes = record_and_submit(&encode_device, &mut self.resources, &mut record_params)
                .map_err(map_err)?;
            self.dpb_transitioned = true;
            (bytes, decision.is_idr)
        };

        let mut payload = self.header_bytes.clone(); // clone: own Packet payload built from the persistent per-session SPS/PPS(+VPS) bytes
        payload.extend_from_slice(&dst_bytes);

        self.pending.push_back(Packet {
            stream_id: self.info.id(),
            pts: frame.pts,
            dts: frame.pts,
            duration: frame.duration,
            is_keyframe,
            is_discard: false,
            payload: Bytes::from(payload),
        });
        Ok(())
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        Ok(self.pending.pop_front())
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        // Every pushed frame is independently encoded and drained
        // synchronously — no pipeline depth to flush.
        self.flushed = true;
        Ok(())
    }
}

impl Drop for VulkanVideoEncoder {
    fn drop(&mut self) {
        // SAFETY: `push_frame` always waits on `resources.fence` synchronously
        // before returning, so no GPU work is outstanding here.
        self.resources.destroy(&self.device_guard.device);
    }
}

fn validate_common(config: &VideoEncoderConfig) -> Result<(), EncodeError> {
    if !matches!(
        config.codec,
        CodecKind::H264 | CodecKind::Hevc | CodecKind::Av1
    ) {
        return Err(EncodeError::Unsupported);
    }
    if config.pixel_format != PixelFormat::Nv12 {
        return Err(EncodeError::Unsupported);
    }
    if !matches!(config.input, VideoInputPreference::CpuUploadOk) {
        return Err(EncodeError::Unsupported);
    }
    if config.width == 0 || config.height == 0 {
        return Err(EncodeError::InvalidInput);
    }
    if config.time_base.den == 0 {
        return Err(EncodeError::InvalidInput);
    }
    // ADR-0002: `0` is rejected at `open()` time (runtime `EncodeError`, not
    // a `debug_assert!`) since `VideoEncoderConfig` is a cross-backend,
    // caller-mutable struct — a release build must reject it too, not
    // silently treat it as "unlimited GOP".
    if config.gop_size == 0 {
        return Err(EncodeError::InvalidInput);
    }
    Ok(())
}

#[allow(clippy::missing_const_for_fn, reason = "StreamInfo holds Bytes")]
fn stream_info_from(config: &VideoEncoderConfig) -> StreamInfo {
    StreamInfo::Video {
        id: 0,
        codec: config.codec,
        time_base: config.time_base,
        geometry: VideoGeometry {
            width: config.width,
            height: config.height,
        },
        extra_data: Bytes::new(),
    }
}

/// Maps this crate's raw Vulkan-call error type to the facade's
/// codec-agnostic [`EncodeError`]. Takes `err` by value so every fallible
/// call site above can pass this function directly to `.map_err(map_err)`.
#[allow(
    clippy::needless_pass_by_value,
    reason = "by-value lets every `?`-using call site pass this directly as \
              `.map_err(map_err)` instead of a `.map_err(|e| map_err(&e))` closure"
)]
fn map_err(err: VulkanEncodeSessionError) -> EncodeError {
    match err {
        VulkanEncodeSessionError::Loader(_)
        | VulkanEncodeSessionError::CreateInstance(_)
        | VulkanEncodeSessionError::EnumeratePhysicalDevices(_)
        | VulkanEncodeSessionError::NoEncodeCapableDevice
        | VulkanEncodeSessionError::NoVideoFormat { .. }
        | VulkanEncodeSessionError::DegenerateCodedExtent { .. } => EncodeError::Unsupported,
        VulkanEncodeSessionError::UnsupportedResolution { .. } => EncodeError::InvalidInput,
        VulkanEncodeSessionError::VkCall { .. } | VulkanEncodeSessionError::NoMemoryType { .. } => {
            EncodeError::Backend
        }
    }
}
