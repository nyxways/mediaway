//! Stage 1 continued (part 2): command recording, submission, and bitstream
//! readback. Split out of `session_encode.rs` only to respect this
//! workspace's 1000-line-per-source-file rule — `session.rs`, `session_encode.rs`,
//! and this file are one logical unit; `session.rs`'s module doc records the
//! real scope and the cuts this stage made.

#![allow(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "Vulkan FFI: every count/size here is driver-reported and small \
              (single digits to low thousands — queue families, DPB slots, \
              memory requirement counts, one coded picture's byte size); casts \
              mirror ash's own generated builder code (e.g. `.len() as _`)."
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "workspace `unreachable_pub` policy (Cargo.toml) wants `pub(crate)` here; \
              clippy::pedantic's redundant_pub_crate disagrees for private modules — the \
              two lints are mutually exclusive for this shape, workspace policy wins"
)]

use vulkanalia::vk;
use vulkanalia::vk::video as native;
use vulkanalia::vk::{
    DeviceV1_0, DeviceV1_3, HasBuilder, KhrVideoEncodeQueueExtensionDeviceCommands,
    KhrVideoQueueExtensionDeviceCommands,
};

use crate::vulkan::h264_params;
use crate::vulkan::session::{EncodeDevice, SessionResources, VulkanEncodeSessionError};

/// DPB slot wiring for one `vkCmdEncodeVideoKHR` call (ADR-0002) —
/// [`Self::idr_only`] reproduces Stage 1's exact single-slot, no-reference
/// shape; GOP mode (`crate::vulkan::h264_gop::GopState`) fills in a real
/// setup/reference slot pair.
pub(crate) struct DpbRecordParams {
    /// How many array layers `resources.dpb_image`/`dpb_image_view` actually
    /// have — `1` for Stage 1's single-slot DPB image, GOP-capped
    /// (`h264_gop::WORKSPACE_DPB_CAP`) otherwise.
    pub(crate) layer_count: u32,
    /// Whether to emit the one-time `UNDEFINED -> VIDEO_ENCODE_DPB_KHR`
    /// layout transition for the whole `dpb_image` this call. `true` for
    /// every Stage 1 call (each is an independent one-shot session that
    /// never reads the DPB back) and for only the *first* `push_frame` of a
    /// GOP-enabled `VulkanVideoEncoder` session — later GOP calls must not
    /// re-discard already-written reference slots (see
    /// [`record_pre_encode_barriers`]).
    pub(crate) transition: bool,
    /// DPB slot this frame's picture is written into
    /// (`VkVideoEncodeInfoKHR::pSetupReferenceSlot`). `0` for Stage 1.
    pub(crate) setup_slot: i32,
    /// `Some` chains a real `VkVideoEncodeH264DpbSlotInfoKHR` onto the setup
    /// slot (GOP mode only — future frames read this back). `None` keeps
    /// Stage 1's exact bare-`VkVideoReferenceSlotInfoKHR` shape, already
    /// hardware-verified without it.
    pub(crate) setup_reference_info: Option<native::StdVideoEncodeH264ReferenceInfo>,
    /// The sole L0 reference to read for a P-frame: the DPB slot index and
    /// that slot's `StdVideoEncodeH264ReferenceInfo`. `None` for an IDR
    /// frame (Stage 1's only case).
    pub(crate) reference: Option<(i32, native::StdVideoEncodeH264ReferenceInfo)>,
}

impl DpbRecordParams {
    /// Stage 1's exact shape: single slot 0, one-time transition every call
    /// (each Stage 1 session only ever calls this once), no reference chain.
    pub(crate) const fn idr_only() -> Self {
        Self {
            layer_count: 1,
            transition: true,
            setup_slot: 0,
            setup_reference_info: None,
            reference: None,
        }
    }
}

/// CBR rate control for one `vkCmdBeginVideoCodingKHR` scope (ADR-0002) —
/// replaces the hardcoded `RATE_CONTROL_MODE_DISABLED` builder when
/// `Some`. Built by the caller (`VulkanVideoEncoder::open`) from
/// `VideoEncoderConfig::rate_control`, once per session (bitrate/framerate
/// do not vary per frame in this design) — `Copy` so
/// `VulkanVideoEncoder::push_frame` can read its stored copy every call
/// without a per-frame clone.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RateControlParams {
    pub(crate) average_bitrate_bps: u64,
    pub(crate) max_bitrate_bps: u64,
    pub(crate) frame_rate_numerator: u32,
    pub(crate) frame_rate_denominator: u32,
    /// `0` lets the driver pick its own default VBV size — see
    /// `VideoEncoderConfig::RateControlConfig::vbv_buffer_size_bytes`'s doc.
    pub(crate) virtual_buffer_size_in_ms: u32,
}

/// Grouped params for [`record_and_submit`] — keeps that function's own
/// signature under `clippy::too_many_arguments`.
pub(crate) struct RecordParams<'a> {
    pub(crate) command_buffer: vk::CommandBuffer,
    pub(crate) coded_extent: vk::Extent2D,
    pub(crate) dst_size: vk::DeviceSize,
    pub(crate) picture_info_pnext: &'a mut vk::VideoEncodeH264PictureInfoKHR,
    pub(crate) dpb: DpbRecordParams,
    pub(crate) rate_control: Option<RateControlParams>,
}

/// Records and submits the whole command buffer: upload barrier + copy,
/// zero-fill the bitstream destination, transition to video-coding layouts,
/// `vkCmdBeginVideoCodingKHR` → `vkCmdEncodeVideoKHR` →
/// `vkCmdEndVideoCodingKHR`, submit, wait, then map and copy back the
/// destination buffer's raw bytes (not trimmed to the driver's actual
/// bytes-written count — see `session.rs`'s module doc "Scope cuts").
///
/// Barriers here are deliberately coarse (`ALL_COMMANDS` / `MEMORY_READ` +
/// `MEMORY_WRITE`) rather than perf-tuned per-stage/access masks — this is a
/// one-shot correctness proof, not a hot path, and coarse sync2 barriers are
/// spec-legal.
pub(crate) fn record_and_submit(
    encode_device: &EncodeDevice<'_>,
    resources: &mut SessionResources,
    params: &mut RecordParams<'_>,
) -> Result<Vec<u8>, VulkanEncodeSessionError> {
    let device = encode_device.device;
    let command_buffer = params.command_buffer;

    // SAFETY: `command_buffer` was allocated from a pool created with
    // `RESET_COMMAND_BUFFER` (see `session_encode::create_command_pool`); no
    // other thread records into or resets it concurrently. A no-op the first
    // time this session ever records (a never-recorded buffer resets to the
    // same empty initial state).
    unsafe { device.reset_command_buffer(command_buffer, vk::CommandBufferResetFlags::empty()) }
        .map_err(|result| VulkanEncodeSessionError::VkCall {
            call: "vkResetCommandBuffer",
            result,
        })?;
    let begin_info =
        vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    // SAFETY: `command_buffer` was just reset above (or freshly allocated,
    // which is an equivalent starting state).
    unsafe { device.begin_command_buffer(command_buffer, &begin_info) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkBeginCommandBuffer",
            result,
        }
    })?;

    record_upload_and_barriers(
        device,
        resources,
        command_buffer,
        params.coded_extent,
        params.dst_size,
        params.dpb.layer_count,
        params.dpb.transition,
    );
    record_video_coding(encode_device, resources, params, command_buffer);

    // SAFETY: `command_buffer` was put into the recording state by
    // `vkBeginCommandBuffer` above and every command since has been valid.
    unsafe { device.end_command_buffer(command_buffer) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkEndCommandBuffer",
            result,
        }
    })?;

    submit_and_readback(encode_device, resources, command_buffer, params.dst_size)
}

/// A whole-image `COLOR`-aspect subresource range — every barrier in this
/// module targets the whole (non-mipmapped, single-layer) image.
const fn whole_color_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
}

/// Upload barrier + `NV12` copy, zero-fill the bitstream destination, then
/// the second barrier batch that transitions both images into their
/// video-coding layouts and the destination buffer into
/// `VIDEO_ENCODE_WRITE_KHR`. See [`record_and_submit`]'s doc for why the
/// barriers are deliberately coarse. `dpb_layer_count`/`transition_dpb` are
/// forwarded to [`record_pre_encode_barriers`] — see that function's doc.
pub(crate) fn record_upload_and_barriers(
    device: &vulkanalia::Device,
    resources: &SessionResources,
    command_buffer: vk::CommandBuffer,
    coded_extent: vk::Extent2D,
    dst_size: vk::DeviceSize,
    dpb_layer_count: u32,
    transition_dpb: bool,
) {
    record_upload(device, resources, command_buffer, coded_extent);
    // SAFETY: `resources.dst_buffer` was just created with `TRANSFER_DST`
    // usage; this is the first command touching it, so no prior barrier is
    // needed before this write.
    unsafe { device.cmd_fill_buffer(command_buffer, resources.dst_buffer, 0, dst_size, 0) };
    record_pre_encode_barriers(
        device,
        resources,
        command_buffer,
        dst_size,
        dpb_layer_count,
        transition_dpb,
    );
}

/// Transitions the input image to `TRANSFER_DST_OPTIMAL` and copies the
/// uploaded `NV12` staging buffer into it (one region per plane).
fn record_upload(
    device: &vulkanalia::Device,
    resources: &SessionResources,
    command_buffer: vk::CommandBuffer,
    coded_extent: vk::Extent2D,
) {
    let whole_color_range = whole_color_range();
    let all_commands = vk::PipelineStageFlags2::ALL_COMMANDS;
    let memory_rw = vk::AccessFlags2::MEMORY_READ | vk::AccessFlags2::MEMORY_WRITE;

    let to_transfer_dst = vk::ImageMemoryBarrier2::builder()
        .src_stage_mask(all_commands)
        .src_access_mask(vk::AccessFlags2::empty())
        .dst_stage_mask(all_commands)
        .dst_access_mask(memory_rw)
        .old_layout(vk::ImageLayout::UNDEFINED)
        .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(resources.input_image)
        .subresource_range(whole_color_range);
    let dep_info =
        vk::DependencyInfo::builder().image_memory_barriers(std::slice::from_ref(&to_transfer_dst));
    // SAFETY: core Vulkan 1.3 call; `dep_info` and its referenced barrier
    // array stay alive for this call.
    unsafe { device.cmd_pipeline_barrier2(command_buffer, &dep_info) };

    let luma_extent = vk::Extent3D {
        width: coded_extent.width,
        height: coded_extent.height,
        depth: 1,
    };
    let chroma_extent = vk::Extent3D {
        width: coded_extent.width / 2,
        height: coded_extent.height / 2,
        depth: 1,
    };
    let luma_bytes =
        vk::DeviceSize::from(coded_extent.width) * vk::DeviceSize::from(coded_extent.height);
    let copy_regions = [
        vk::BufferImageCopy::builder()
            .buffer_offset(0)
            .image_subresource(vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::PLANE_0,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            })
            .image_extent(luma_extent),
        vk::BufferImageCopy::builder()
            .buffer_offset(luma_bytes)
            .image_subresource(vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::PLANE_1,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            })
            .image_extent(chroma_extent),
    ];
    // SAFETY: `resources.staging_buffer`/`resources.input_image` are both
    // valid, `input_image` was just transitioned to `TRANSFER_DST_OPTIMAL`
    // above, and `copy_regions` covers exactly the bytes uploaded into the
    // staging buffer by `upload_to_host_memory`.
    unsafe {
        device.cmd_copy_buffer_to_image(
            command_buffer,
            resources.staging_buffer,
            resources.input_image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &copy_regions,
        );
    }
}

/// Transitions the (already-filled) input image into `VIDEO_ENCODE_SRC_KHR`,
/// the DPB image into `VIDEO_ENCODE_DPB_KHR`, and the (already zero-filled)
/// destination buffer's access mask into `VIDEO_ENCODE_WRITE_KHR` — one
/// batched `vkCmdPipelineBarrier2` call.
///
/// `dpb_layer_count` sizes the DPB barrier's subresource range (ADR-0002's
/// GOP mode uses a multi-layer `dpb_image`, one layer per DPB slot — Stage 1
/// stays `1`). `transition_dpb` chooses the DPB barrier's `old_layout`:
/// `UNDEFINED` (discard — correct only the *first* time each layer is ever
/// touched) when `true`, or a same-layout `VIDEO_ENCODE_DPB_KHR ->
/// VIDEO_ENCODE_DPB_KHR` no-op when `false` — GOP mode passes `false` after
/// its first call so already-written reference slots are never discarded;
/// Stage 1 always passes `true` (each session only ever calls this once, so
/// there is nothing valid to preserve).
fn record_pre_encode_barriers(
    device: &vulkanalia::Device,
    resources: &SessionResources,
    command_buffer: vk::CommandBuffer,
    dst_size: vk::DeviceSize,
    dpb_layer_count: u32,
    transition_dpb: bool,
) {
    let whole_color_range = whole_color_range();
    let all_commands = vk::PipelineStageFlags2::ALL_COMMANDS;
    let memory_rw = vk::AccessFlags2::MEMORY_READ | vk::AccessFlags2::MEMORY_WRITE;

    let to_encode_src = vk::ImageMemoryBarrier2::builder()
        .src_stage_mask(all_commands)
        .src_access_mask(memory_rw)
        .dst_stage_mask(all_commands)
        .dst_access_mask(memory_rw)
        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .new_layout(vk::ImageLayout::VIDEO_ENCODE_SRC_KHR)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(resources.input_image)
        .subresource_range(whole_color_range);
    let dpb_old_layout = if transition_dpb {
        vk::ImageLayout::UNDEFINED
    } else {
        vk::ImageLayout::VIDEO_ENCODE_DPB_KHR
    };
    let dpb_range = vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: dpb_layer_count,
    };
    let dpb_to_encode = vk::ImageMemoryBarrier2::builder()
        .src_stage_mask(all_commands)
        .src_access_mask(vk::AccessFlags2::empty())
        .dst_stage_mask(all_commands)
        .dst_access_mask(memory_rw)
        .old_layout(dpb_old_layout)
        .new_layout(vk::ImageLayout::VIDEO_ENCODE_DPB_KHR)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(resources.dpb_image)
        .subresource_range(dpb_range);
    let dst_buffer_barrier = vk::BufferMemoryBarrier2::builder()
        .src_stage_mask(all_commands)
        .src_access_mask(memory_rw)
        .dst_stage_mask(all_commands)
        .dst_access_mask(memory_rw)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .buffer(resources.dst_buffer)
        .offset(0)
        .size(dst_size);
    let image_barriers = [to_encode_src, dpb_to_encode];
    let dep_info = vk::DependencyInfo::builder()
        .image_memory_barriers(&image_barriers)
        .buffer_memory_barriers(std::slice::from_ref(&dst_buffer_barrier));
    // SAFETY: core Vulkan 1.3 call; all referenced barrier arrays stay alive
    // for this call.
    unsafe { device.cmd_pipeline_barrier2(command_buffer, &dep_info) };
}

/// `vkCmdBeginVideoCodingKHR` → `vkCmdEncodeVideoKHR` →
/// `vkCmdEndVideoCodingKHR` for one frame. `params.dpb ==
/// DpbRecordParams::idr_only()` (Stage 1's only case) reproduces the
/// original single-slot, no-reference, `DISABLED`-rate-control call shape
/// exactly, field for field; ADR-0002's GOP mode instead wires a real
/// setup/reference slot pair (with `VkVideoEncodeH264DpbSlotInfoKHR` chained
/// so a *future* frame can read this one back, mirroring the AV1 addendum's
/// same finding in `adr/0001`) and, when `params.rate_control.is_some()`,
/// `CBR` instead of `DISABLED`.
#[allow(
    clippy::too_many_lines,
    reason = "linear per-frame Vulkan Video struct construction — every local here \
              (setup/reference slot resources, DPB-slot-info chains, rate control) must \
              stay on this function's own stack frame for the raw pointers built from them \
              to remain valid through the unsafe calls at the bottom; splitting further would \
              just move half these locals into an out-param struct with the same lifetime need"
)]
fn record_video_coding(
    encode_device: &EncodeDevice<'_>,
    resources: &SessionResources,
    params: &mut RecordParams<'_>,
    command_buffer: vk::CommandBuffer,
) {
    let device = encode_device.device;

    // --- setup slot: the DPB slot this frame's picture is written into ---
    let setup_resource = vk::VideoPictureResourceInfoKHR::builder()
        .coded_offset(vk::Offset2D { x: 0, y: 0 })
        .coded_extent(params.coded_extent)
        .base_array_layer(params.dpb.setup_slot as u32)
        .image_view_binding(resources.dpb_image_view);
    let setup_reference_info = params
        .dpb
        .setup_reference_info
        .unwrap_or_else(|| h264_params::build_reference_info(0, 0, true));
    let mut setup_dpb_slot_info = vk::VideoEncodeH264DpbSlotInfoKHR::builder()
        .std_reference_info(&setup_reference_info)
        .build();
    let setup_slot = if params.dpb.setup_reference_info.is_some() {
        vk::VideoReferenceSlotInfoKHR::builder()
            .slot_index(params.dpb.setup_slot)
            .picture_resource(&setup_resource)
            .push_next(&mut setup_dpb_slot_info)
    } else {
        vk::VideoReferenceSlotInfoKHR::builder()
            .slot_index(params.dpb.setup_slot)
            .picture_resource(&setup_resource)
    };
    // `begin_slots`' own setup-slot entry stays exactly Stage 1's shape (no
    // `picture_resource`, no DPB-slot-info chain) — this slot's content is
    // being (re)initialized by this very operation, matching the
    // already-hardware-verified convention.
    let begin_setup_slot =
        vk::VideoReferenceSlotInfoKHR::builder().slot_index(params.dpb.setup_slot);

    // --- reference slot: the sole L0 reference a P-frame reads (GOP only) ---
    let has_reference = params.dpb.reference.is_some();
    let (reference_slot_index, reference_std_info) = params
        .dpb
        .reference
        .unwrap_or_else(|| (0, h264_params::build_reference_info(0, 0, true)));
    let reference_resource = vk::VideoPictureResourceInfoKHR::builder()
        .coded_offset(vk::Offset2D { x: 0, y: 0 })
        .coded_extent(params.coded_extent)
        .base_array_layer(reference_slot_index as u32)
        .image_view_binding(resources.dpb_image_view);
    // Two separate `StdVideoEncodeH264DpbSlotInfoKHR` locals (not one reused
    // for both arrays below): each `push_next` call takes an exclusive
    // borrow, and both the `begin_slots` and `encode_info.reference_slots`
    // arrays must stay alive simultaneously through the unsafe calls below.
    let mut begin_reference_dpb_slot_info = vk::VideoEncodeH264DpbSlotInfoKHR::builder()
        .std_reference_info(&reference_std_info)
        .build();
    let mut encode_reference_dpb_slot_info = vk::VideoEncodeH264DpbSlotInfoKHR::builder()
        .std_reference_info(&reference_std_info)
        .build();
    let begin_reference_slot = vk::VideoReferenceSlotInfoKHR::builder()
        .slot_index(reference_slot_index)
        .picture_resource(&reference_resource)
        .push_next(&mut begin_reference_dpb_slot_info);
    let encode_reference_slot = vk::VideoReferenceSlotInfoKHR::builder()
        .slot_index(reference_slot_index)
        .picture_resource(&reference_resource)
        .push_next(&mut encode_reference_dpb_slot_info);
    let begin_slots_array = [begin_setup_slot, begin_reference_slot];
    let begin_slots = if has_reference {
        &begin_slots_array[..]
    } else {
        &begin_slots_array[..1]
    };
    let encode_reference_slots_array = [encode_reference_slot];

    // --- rate control: `DISABLED` fixed-QP (Stage 1's exact shape) or CBR ---
    let rate_control_layer = params
        .rate_control
        .as_ref()
        .map(|rc| {
            vk::VideoEncodeRateControlLayerInfoKHR::builder()
                .average_bitrate(rc.average_bitrate_bps)
                .max_bitrate(rc.max_bitrate_bps)
                .frame_rate_numerator(rc.frame_rate_numerator)
                .frame_rate_denominator(rc.frame_rate_denominator)
                .build()
        })
        .unwrap_or_default();
    let rate_control_layers = [rate_control_layer];

    // SAFETY: `resources.encode_feedback_query_pool` has exactly 1 query slot
    // (`create_encode_feedback_query_pool`); resetting before use is required
    // — a query pool's slots start in an undefined state.
    unsafe {
        device.cmd_reset_query_pool(command_buffer, resources.encode_feedback_query_pool, 0, 1);
    }
    let mut rate_control = params.rate_control.as_ref().map_or_else(
        || {
            vk::VideoEncodeRateControlInfoKHR::builder()
                .rate_control_mode(vk::VideoEncodeRateControlModeFlagsKHR::DISABLED)
        },
        |rc| {
            vk::VideoEncodeRateControlInfoKHR::builder()
                .rate_control_mode(vk::VideoEncodeRateControlModeFlagsKHR::CBR)
                .layers(&rate_control_layers)
                .virtual_buffer_size_in_ms(rc.virtual_buffer_size_in_ms)
                .initial_virtual_buffer_size_in_ms(rc.virtual_buffer_size_in_ms)
        },
    );
    let begin_info = vk::VideoBeginCodingInfoKHR::builder()
        .video_session(resources.session)
        .video_session_parameters(resources.session_parameters)
        .reference_slots(begin_slots)
        .push_next(&mut rate_control);
    // SAFETY: `begin_info` and its chained `rate_control`/`begin_slots` (and
    // everything `begin_slots`' entries themselves chain) stay alive for
    // this call; `command_buffer` is currently recording.
    unsafe {
        device.cmd_begin_video_coding_khr(command_buffer, &begin_info);
    }

    let src_resource = vk::VideoPictureResourceInfoKHR::builder()
        .coded_offset(vk::Offset2D { x: 0, y: 0 })
        .coded_extent(params.coded_extent)
        .base_array_layer(0)
        .image_view_binding(resources.input_image_view);
    let encode_info = if has_reference {
        vk::VideoEncodeInfoKHR::builder()
            .dst_buffer(resources.dst_buffer)
            .dst_buffer_offset(0)
            .dst_buffer_range(params.dst_size)
            .src_picture_resource(src_resource)
            .setup_reference_slot(&setup_slot)
            .reference_slots(&encode_reference_slots_array)
            .push_next(params.picture_info_pnext)
    } else {
        vk::VideoEncodeInfoKHR::builder()
            .dst_buffer(resources.dst_buffer)
            .dst_buffer_offset(0)
            .dst_buffer_range(params.dst_size)
            .src_picture_resource(src_resource)
            .setup_reference_slot(&setup_slot)
            .push_next(params.picture_info_pnext)
    };
    // SAFETY: `command_buffer` is inside the active video-coding scope opened
    // above; query 0 was just reset and is not currently active.
    unsafe {
        device.cmd_begin_query(
            command_buffer,
            resources.encode_feedback_query_pool,
            0,
            vk::QueryControlFlags::empty(),
        );
    }
    // SAFETY: `encode_info` and everything it chains/borrows
    // (`src_resource`/`setup_slot`/`encode_reference_slots_array`/the H.264
    // picture-info chain built by the caller) stay alive for this call;
    // `command_buffer` is inside an active video-coding scope from the
    // `cmd_begin_video_coding_khr` call immediately above.
    unsafe {
        device.cmd_encode_video_khr(command_buffer, &encode_info);
    }
    // SAFETY: matches the `cmd_begin_query` immediately above, same
    // `command_buffer`, still inside the video-coding scope.
    unsafe {
        device.cmd_end_query(command_buffer, resources.encode_feedback_query_pool, 0);
    }

    let end_info = vk::VideoEndCodingInfoKHR::builder();
    // SAFETY: ends the video-coding scope opened above on this same
    // `command_buffer`.
    unsafe {
        device.cmd_end_video_coding_khr(command_buffer, &end_info);
    }
}

/// Submits the recorded command buffer, waits for it to finish, then maps
/// and copies back `resources.dst_buffer`'s raw bytes.
pub(crate) fn submit_and_readback(
    encode_device: &EncodeDevice<'_>,
    resources: &mut SessionResources,
    command_buffer: vk::CommandBuffer,
    dst_size: vk::DeviceSize,
) -> Result<Vec<u8>, VulkanEncodeSessionError> {
    let device = encode_device.device;
    // SAFETY: `resources.fence` was created once by the caller
    // (`session_encode::create_fence`) and is not currently in use by any
    // pending GPU work — the previous call's `wait_for_fences` below (if any)
    // already guaranteed that. A no-op on a fence that has never been
    // signaled (this call's first use).
    unsafe { device.reset_fences(&[resources.fence]) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkResetFences",
            result,
        }
    })?;
    let cb_submit_info = vk::CommandBufferSubmitInfo::builder()
        .command_buffer(command_buffer)
        .device_mask(0);
    let submit_info =
        vk::SubmitInfo2::builder().command_buffer_infos(std::slice::from_ref(&cb_submit_info));
    // SAFETY: `command_buffer` finished recording above; `resources.fence`
    // was just created, unsignaled.
    unsafe { device.queue_submit2(encode_device.queue, &[submit_info], resources.fence) }.map_err(
        |result| VulkanEncodeSessionError::VkCall {
            call: "vkQueueSubmit2",
            result,
        },
    )?;
    // SAFETY: `resources.fence` was submitted with the queue submission
    // above.
    unsafe { device.wait_for_fences(&[resources.fence], true, u64::MAX) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkWaitForFences",
            result,
        }
    })?;

    // SAFETY: `resources.dst_memory` is bound to `resources.dst_buffer`
    // (`create_host_buffer`), sized `dst_size`; the fence wait above
    // guarantees the GPU's writes to it are complete; nothing else has it
    // mapped.
    let ptr = unsafe {
        device.map_memory(
            resources.dst_memory,
            0,
            dst_size,
            vk::MemoryMapFlags::empty(),
        )
    }
    .map_err(|result| VulkanEncodeSessionError::VkCall {
        call: "vkMapMemory (dst)",
        result,
    })?;
    // SAFETY: `ptr` is valid for `dst_size` bytes, just mapped above; memory
    // type is `HOST_COHERENT` so its contents are visible now that the fence
    // has signaled.
    let dst_bytes =
        unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), dst_size as usize) }.to_vec();
    // SAFETY: `resources.dst_memory` was mapped by this same function
    // immediately above.
    unsafe { device.unmap_memory(resources.dst_memory) };

    // The fence wait above already guarantees the query's result is
    // available — `QueryResultFlags::WAIT` is defensive, not load-bearing.
    let mut bytes_written = [0u32; 1];
    // SAFETY: `resources.encode_feedback_query_pool` has exactly 1 query
    // slot, written by the `cmd_begin_query`/`cmd_end_query`-bracketed
    // `vkCmdEncodeVideoKHR` in the command buffer just submitted and waited
    // on; requesting only `BITSTREAM_BYTES_WRITTEN` (see
    // `session_encode::create_encode_feedback_query_pool`) means the result
    // is a single tightly-packed `u32`, matching `bytes_written`'s layout —
    // reinterpreted as its 4-byte view for this trait method's `&mut [u8]`
    // signature.
    let bytes_written_view = unsafe {
        std::slice::from_raw_parts_mut(
            bytes_written.as_mut_ptr().cast::<u8>(),
            std::mem::size_of_val(&bytes_written),
        )
    };
    unsafe {
        device.get_query_pool_results(
            resources.encode_feedback_query_pool,
            0,
            1,
            bytes_written_view,
            vk::DeviceSize::try_from(std::mem::size_of::<u32>()).unwrap_or(4),
            vk::QueryResultFlags::WAIT,
        )
    }
    .map_err(|result| VulkanEncodeSessionError::VkCall {
        call: "vkGetQueryPoolResults",
        result,
    })?;
    let written = usize::try_from(bytes_written[0])
        .unwrap_or(0)
        .min(dst_bytes.len());

    Ok(dst_bytes[..written].to_vec())
}
