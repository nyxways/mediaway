//! Public [`mediaway_encoder::VideoEncoder`] entry point: a reusable,
//! multi-frame `VulkanVideoEncoder` (H.264 or HEVC) built on the Stage 1
//! session machinery in
//! [`crate::session`]/[`crate::session_encode`]/[`crate::session_command`]/
//! [`crate::session_command_hevc`].
//!
//! Unlike [`crate::session_encode::encode_synthetic_intra_frame`] (a one-shot
//! diagnostic that builds and tears down a whole session for a single
//! synthetic frame), this type keeps the instance/device/video session/
//! session parameters/images/buffers/command pool/fence/query pool alive
//! across every [`VideoEncoder::push_frame`] call — only the per-frame
//! upload/record/submit/readback repeats, mirroring
//! `mediaway-encoder-windows`'s `D3d12VideoEncoder` session shape. CPU-upload
//! NV12 input only (this crate's Stage 3 Zero-Copy external-memory import is
//! still deferred); every pushed frame is an independent key frame (no GOP,
//! no P/B-frames, no DPB reference reuse — same scope cut as Stage 1).

#![allow(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    reason = "idr_pic_id is a wrapping u16 counter — mirrors session_encode.rs's/\
              session_command.rs's crate-wide allow for small driver-facing counts"
)]

use std::collections::VecDeque;

use mediaway_common::{
    Bytes, CodecKind, Packet, PixelFormat, StreamInfo, VideoFrame, VideoFrameStorage, VideoGeometry,
};
use mediaway_encoder::{EncodeError, VideoEncoder, VideoEncoderConfig, VideoInputPreference};
use vulkanalia::vk;
use vulkanalia::vk::{DeviceV1_0, HasBuilder, InstanceV1_0};

use crate::av1_params;
use crate::h264_params::{self, McAlignedExtent};
use crate::hevc_params::{self, CtuAlignedExtent};
use crate::session::{
    DeviceGuard, EncodeDevice, EncodeProfile, InstanceGuard, SessionResources,
    VulkanEncodeSessionError, create_instance, create_logical_device, find_av1_encode_device,
    find_h264_encode_device, find_hevc_encode_device, query_capabilities, query_video_format,
};
use crate::session_command::{RecordParams, record_and_submit};
use crate::session_command_av1::{RecordParamsAv1, record_and_submit_av1};
use crate::session_command_hevc::{RecordParamsHevc, record_and_submit_hevc};
use crate::session_encode::{
    allocate_command_buffer, create_command_pool, create_encode_feedback_query_pool, create_fence,
    create_images_and_buffers, create_session_parameters, create_session_parameters_av1,
    create_session_parameters_hevc, create_video_session, get_encoded_headers,
    get_encoded_headers_av1, get_encoded_headers_hevc, nv12_byte_size, upload_to_host_memory,
};

/// Fixed intra `constant_qp` (all-intra CQP) — rate-control tuning is
/// deferred, mirrors `mediaway-encoder-windows`'s D3D12 backend's `FIXED_QP`.
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
    ///   `time_base` denominator, or `config.width`/`height` falls outside
    ///   this driver's reported coded-extent bounds/alignment.
    /// - [`EncodeError::Backend`] — a Vulkan object-creation call failed.
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
            let dpb_mgr = hevc_params::dec_pic_buf_mgr_no_refs();
            let vps = hevc_params::build_vps(&ptl, &dpb_mgr);
            let sps = hevc_params::build_sps(extent, &ptl, &dpb_mgr);
            let pps = hevc_params::build_pps();
            resources.session_parameters =
                create_session_parameters_hevc(&encode_device, session, &vps, &sps, &pps)
                    .map_err(map_err)?;
            get_encoded_headers_hevc(&encode_device, resources.session_parameters)
                .map_err(map_err)?
        } else {
            let extent = McAlignedExtent::from_pixels(coded_extent.width, coded_extent.height)
                .ok_or(EncodeError::InvalidInput)?;
            let sps = h264_params::build_sps(extent);
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
        let dst_bytes = if self.codec == CodecKind::Av1 {
            let optionals = av1_params::PictureInfoOptionals::new();
            let picture_info = av1_params::build_key_frame_picture_info(
                self.coded_extent.width,
                self.coded_extent.height,
                &optionals,
            );
            let mut av1_picture_info = vk::VideoEncodeAV1PictureInfoKHR::builder()
                .prediction_mode(vk::VideoEncodeAV1PredictionModeKHR::VIDEO_ENCODE_AV1_PREDICTION_MODE_INTRA_ONLY)
                .rate_control_group(vk::VideoEncodeAV1RateControlGroupKHR::VIDEO_ENCODE_AV1_RATE_CONTROL_GROUP_INTRA)
                .constant_q_index(u32::from(av1_params::FIXED_Q_INDEX))
                .std_picture_info(&picture_info)
                .reference_name_slot_indices([-1; 7])
                .primary_reference_cdf_only(false)
                .generate_obu_extension_header(false)
                .build();
            self.frame_counter = self.frame_counter.wrapping_add(1);

            let mut record_params = RecordParamsAv1 {
                command_buffer: self.command_buffer,
                coded_extent: self.coded_extent,
                dst_size: self.dst_size,
                picture_info_pnext: &mut av1_picture_info,
            };
            record_and_submit_av1(&encode_device, &mut self.resources, &mut record_params)
                .map_err(map_err)?
        } else if self.codec == CodecKind::Hevc {
            let ref_lists = hevc_params::build_empty_reference_lists();
            let mut picture_info = hevc_params::build_idr_picture_info();
            picture_info.pRefLists = &raw const ref_lists;
            let slice_header = hevc_params::build_idr_slice_segment_header();
            let nalu_slice_entries = [vk::VideoEncodeH265NaluSliceSegmentInfoKHR::builder()
                .constant_qp(FIXED_QP)
                .std_slice_segment_header(&slice_header)
                .build()];
            let mut hevc_picture_info = vk::VideoEncodeH265PictureInfoKHR::builder()
                .nalu_slice_segment_entries(&nalu_slice_entries)
                .std_picture_info(&picture_info)
                .build();
            self.frame_counter = self.frame_counter.wrapping_add(1);

            let mut record_params = RecordParamsHevc {
                command_buffer: self.command_buffer,
                coded_extent: self.coded_extent,
                dst_size: self.dst_size,
                picture_info_pnext: &mut hevc_picture_info,
            };
            record_and_submit_hevc(&encode_device, &mut self.resources, &mut record_params)
                .map_err(map_err)?
        } else {
            let ref_lists = h264_params::build_empty_reference_lists();
            let mut picture_info = h264_params::build_idr_picture_info();
            picture_info.idr_pic_id = self.frame_counter as u16;
            picture_info.pRefLists = &raw const ref_lists;
            let slice_header = h264_params::build_idr_slice_header();
            let nalu_slice_entries = [vk::VideoEncodeH264NaluSliceInfoKHR::builder()
                .constant_qp(FIXED_QP)
                .std_slice_header(&slice_header)
                .build()];
            let mut h264_picture_info = vk::VideoEncodeH264PictureInfoKHR::builder()
                .nalu_slice_entries(&nalu_slice_entries)
                .std_picture_info(&picture_info)
                .generate_prefix_nalu(false)
                .build();
            self.frame_counter = self.frame_counter.wrapping_add(1);

            let mut record_params = RecordParams {
                command_buffer: self.command_buffer,
                coded_extent: self.coded_extent,
                dst_size: self.dst_size,
                picture_info_pnext: &mut h264_picture_info,
            };
            record_and_submit(&encode_device, &mut self.resources, &mut record_params)
                .map_err(map_err)?
        };

        let mut payload = self.header_bytes.clone(); // clone: own Packet payload built from the persistent per-session SPS/PPS(+VPS) bytes
        payload.extend_from_slice(&dst_bytes);

        self.pending.push_back(Packet {
            stream_id: self.info.id(),
            pts: frame.pts,
            dts: frame.pts,
            duration: frame.duration,
            is_keyframe: true,
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
