//! AV1-specific `vkCmdDecodeVideoKHR` picture-info `pNext` payload
//! construction, plus the AV1 `record_and_submit_av1` entry point built on
//! `session_command.rs`'s shared [`SessionResources`] — mirrors
//! `session_command_h264.rs`/`session_command_hevc.rs`'s shape, reusing the
//! same three real Vulkan Video protocol requirements those two modules
//! already found on real hardware (`adr/vulkan/0001`'s 2026-07-30 addenda):
//! the setup slot's `slotIndex = -1` at `vkCmdBeginVideoCodingKHR`, the
//! destination layer's `VIDEO_DECODE_DPB_KHR` ⇄ `VIDEO_DECODE_DST_KHR`
//! transition around the decode command, and `submit_and_wait`. These are
//! Vulkan-level protocol requirements, not codec-specific, so they are
//! reused as-is rather than rediscovered here (`adr/vulkan/0002`'s own file
//! layout plan).
//!
//! **AV1-specific, unlike H.264/HEVC**: no Annex-B start code is prepended —
//! `resources.bitstream_buffer` holds the `OBU_FRAME`'s raw payload bytes
//! (see `av1_frame_header.rs`'s module doc for the `frameHeaderOffset`/tile-
//! offset design this implies); `VkVideoDecodeAV1PictureInfoKHR`'s
//! `reference_name_slot_indices`/`frame_header_offset`/`tile_count`/
//! `pTileOffsets`/`pTileSizes` replace H.264/HEVC's `slice_offsets`/
//! `slice_segment_offsets`.

#![allow(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "Vulkan FFI: every count/size here is driver-reported and small — casts mirror \
              session_command_hevc.rs's identical allow."
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "workspace `unreachable_pub` policy (Cargo.toml) wants `pub(crate)` here; \
              clippy::pedantic's redundant_pub_crate disagrees for private modules — the \
              two lints are mutually exclusive for this shape, workspace policy wins"
)]
#![allow(
    clippy::too_many_lines,
    reason = "linear per-frame command-recording sequence (barriers -> begin video coding -> \
              decode -> end video coding -> submit), mirrors session_command_hevc.rs's own \
              record_and_submit shape"
)]

use vulkanalia::vk;
use vulkanalia::vk::{
    DeviceV1_0, DeviceV1_3, HasBuilder, KhrVideoDecodeQueueExtensionDeviceCommands,
    KhrVideoQueueExtensionDeviceCommands,
};

use crate::vulkan::av1_params::{Av1FrameHeader, Av1PictureInfoOptionals};
use crate::vulkan::session::{DecodeDevice, VulkanDecodeError};
use crate::vulkan::session_command::{SessionResources, color_range_for_layer, submit_and_wait};

/// Runs the one-time `vkCmdControlVideoCodingKHR` `RESET` — identical shape
/// to `session_command_h264::reset_session_once`/
/// `session_command_hevc::reset_session_once` (duplicated rather than
/// shared, same reasoning those two modules' own doc already gives:
/// `SessionResources::session_reset` tracks it once per session regardless
/// of codec, and each codec's own `record_and_submit_*` calls it first).
fn reset_session_once(
    decode_device: &DecodeDevice<'_>,
    resources: &mut SessionResources,
    command_buffer: vk::CommandBuffer,
) -> Result<(), VulkanDecodeError> {
    if resources.session_reset {
        return Ok(());
    }
    let device = decode_device.device;
    // SAFETY: `command_buffer` was allocated from a pool created with
    // `RESET_COMMAND_BUFFER`; a no-op the first time this session ever
    // records (a never-recorded buffer resets to the same empty state).
    unsafe { device.reset_command_buffer(command_buffer, vk::CommandBufferResetFlags::empty()) }
        .map_err(|result| VulkanDecodeError::VkCall {
            call: "vkResetCommandBuffer",
            result,
        })?;
    let begin_info =
        vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    // SAFETY: `command_buffer` was just reset above.
    unsafe { device.begin_command_buffer(command_buffer, &begin_info) }.map_err(|result| {
        VulkanDecodeError::VkCall {
            call: "vkBeginCommandBuffer",
            result,
        }
    })?;

    let begin_coding = vk::VideoBeginCodingInfoKHR::builder()
        .video_session(resources.session)
        .video_session_parameters(resources.session_parameters);
    // SAFETY: `begin_coding` is valid for this call; `command_buffer` is
    // recording.
    unsafe { device.cmd_begin_video_coding_khr(command_buffer, &begin_coding) };

    let control_info =
        vk::VideoCodingControlInfoKHR::builder().flags(vk::VideoCodingControlFlagsKHR::RESET);
    // SAFETY: `control_info` valid; `command_buffer` is inside the
    // video-coding scope opened above.
    unsafe { device.cmd_control_video_coding_khr(command_buffer, &control_info) };

    let end_info = vk::VideoEndCodingInfoKHR::builder();
    // SAFETY: ends the scope opened above on this same `command_buffer`.
    unsafe { device.cmd_end_video_coding_khr(command_buffer, &end_info) };

    // SAFETY: every command above was recorded into `command_buffer`, which
    // is currently in the recording state.
    unsafe { device.end_command_buffer(command_buffer) }.map_err(|result| {
        VulkanDecodeError::VkCall {
            call: "vkEndCommandBuffer",
            result,
        }
    })?;
    submit_and_wait(decode_device, resources, command_buffer)?;
    resources.session_reset = true;
    Ok(())
}

/// Everything [`record_and_submit_av1`] needs for one picture's decode
/// command. Grouped to stay under `clippy::too_many_arguments` — mirrors
/// `session_command_hevc::RecordParamsHevc`.
pub(crate) struct RecordParamsAv1<'a> {
    pub(crate) command_buffer: vk::CommandBuffer,
    pub(crate) coded_extent: vk::Extent2D,
    /// Bytes of `resources.bitstream_buffer` actually holding this picture's
    /// `OBU_FRAME` payload (no Annex-B start code — see the module doc).
    pub(crate) bitstream_len: vk::DeviceSize,
    /// DPB array-layer index this picture decodes into.
    pub(crate) dst_slot_index: u32,
    /// Byte offset (within the uploaded `OBU_FRAME` payload) where the
    /// single tile's coded data begins.
    pub(crate) tile_offset: u32,
    /// Byte length of the single tile's coded data.
    pub(crate) tile_size: u32,
    pub(crate) frame_header: &'a Av1FrameHeader,
    pub(crate) optionals: &'a Av1PictureInfoOptionals,
}

/// Records and submits one `KEY_FRAME`'s `vkCmdDecodeVideoKHR`, waits for
/// completion, then leaves the decoded picture in `resources.dpb_image`'s
/// `params.dst_slot_index` layer for the caller to read back
/// ([`crate::vulkan::cpu_readback`]) or hand out as a Zero-Copy handle
/// ([`crate::vulkan::zero_copy`]) — mirrors `session_command_hevc::record_and_submit_hevc`.
///
/// `params.optionals` must already have had
/// [`Av1PictureInfoOptionals::finish`] called on it (its `StdVideoAV1TileInfo`
/// array pointers are only valid once `optionals` has its final stack
/// address — see that method's own doc); this function does not call it
/// itself so the caller controls exactly when `optionals` stops moving.
pub(crate) fn record_and_submit_av1(
    decode_device: &DecodeDevice<'_>,
    resources: &mut SessionResources,
    params: &RecordParamsAv1<'_>,
) -> Result<(), VulkanDecodeError> {
    let device = decode_device.device;
    let command_buffer = params.command_buffer;
    reset_session_once(decode_device, resources, command_buffer)?;

    // SAFETY: `command_buffer` was allocated from a pool created with
    // `RESET_COMMAND_BUFFER`.
    unsafe { device.reset_command_buffer(command_buffer, vk::CommandBufferResetFlags::empty()) }
        .map_err(|result| VulkanDecodeError::VkCall {
            call: "vkResetCommandBuffer",
            result,
        })?;
    let begin_info =
        vk::CommandBufferBeginInfo::builder().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    // SAFETY: `command_buffer` was just reset above.
    unsafe { device.begin_command_buffer(command_buffer, &begin_info) }.map_err(|result| {
        VulkanDecodeError::VkCall {
            call: "vkBeginCommandBuffer",
            result,
        }
    })?;

    // Host-write-to-device-read barrier for the bitstream buffer, plus the
    // destination layer's `VIDEO_DECODE_DPB_KHR` -> `VIDEO_DECODE_DST_KHR`
    // transition — see `session_command_h264.rs`'s identical barrier for the
    // real-hardware finding this mirrors.
    let all_commands = vk::PipelineStageFlags2::ALL_COMMANDS;
    let memory_rw = vk::AccessFlags2::MEMORY_READ | vk::AccessFlags2::MEMORY_WRITE;
    let bitstream_barrier = vk::BufferMemoryBarrier2::builder()
        .src_stage_mask(vk::PipelineStageFlags2::HOST)
        .src_access_mask(vk::AccessFlags2::HOST_WRITE)
        .dst_stage_mask(all_commands)
        .dst_access_mask(memory_rw)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .buffer(resources.bitstream_buffer)
        .offset(0)
        .size(params.bitstream_len);
    let dst_range = color_range_for_layer(params.dst_slot_index);
    let to_decode_dst = vk::ImageMemoryBarrier2::builder()
        .src_stage_mask(all_commands)
        .src_access_mask(memory_rw)
        .dst_stage_mask(vk::PipelineStageFlags2::VIDEO_DECODE_KHR)
        .dst_access_mask(vk::AccessFlags2::VIDEO_DECODE_WRITE_KHR)
        .old_layout(vk::ImageLayout::VIDEO_DECODE_DPB_KHR)
        .new_layout(vk::ImageLayout::VIDEO_DECODE_DST_KHR)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(resources.dpb_image)
        .subresource_range(dst_range);
    let dep_info = vk::DependencyInfo::builder()
        .buffer_memory_barriers(std::slice::from_ref(&bitstream_barrier))
        .image_memory_barriers(std::slice::from_ref(&to_decode_dst));
    // SAFETY: core Vulkan 1.3 call; `dep_info` and its referenced barriers
    // stay alive for this call.
    unsafe { device.cmd_pipeline_barrier2(command_buffer, &dep_info) };

    let dst_resource = vk::VideoPictureResourceInfoKHR::builder()
        .coded_offset(vk::Offset2D { x: 0, y: 0 })
        .coded_extent(params.coded_extent)
        .base_array_layer(params.dst_slot_index)
        .image_view_binding(resources.dpb_image_view)
        .build();
    let dst_reference_info = params.frame_header.to_std_reference_info();
    let mut dst_dpb_slot_info = vk::VideoDecodeAV1DpbSlotInfoKHR::builder()
        .std_reference_info(&dst_reference_info)
        .build();
    let setup_slot = vk::VideoReferenceSlotInfoKHR::builder()
        .slot_index(params.dst_slot_index as i32)
        .picture_resource(&dst_resource)
        .push_next(&mut dst_dpb_slot_info)
        .build();

    // vkCmdBeginVideoCodingKHR: the setup slot is introduced with
    // `slotIndex = -1` (see `session_command_h264.rs`'s identical comment for
    // the real-hardware finding this mirrors).
    let mut begin_setup_slot = setup_slot;
    begin_setup_slot.slot_index = -1;
    let begin_slots = [begin_setup_slot];
    let begin_coding = vk::VideoBeginCodingInfoKHR::builder()
        .video_session(resources.session)
        .video_session_parameters(resources.session_parameters)
        .reference_slots(&begin_slots);
    // SAFETY: `begin_coding` and its referenced slot array stay alive for
    // this call; `command_buffer` is recording.
    unsafe { device.cmd_begin_video_coding_khr(command_buffer, &begin_coding) };

    let std_picture_info = params.frame_header.to_std_picture_info(params.optionals);
    let reference_name_slot_indices = [-1i32; 7];
    let tile_offsets = [params.tile_offset];
    let tile_sizes = [params.tile_size];
    let mut av1_picture_info = vk::VideoDecodeAV1PictureInfoKHR::builder()
        .std_picture_info(&std_picture_info)
        .reference_name_slot_indices(reference_name_slot_indices)
        .frame_header_offset(0)
        .tile_offsets(&tile_offsets)
        .tile_sizes(&tile_sizes)
        .build();
    // KEY_FRAME reads no reference (every `reference_name_slot_indices`
    // entry is -1) — the empty `reference_slots` array below is distinct
    // from `begin_slots` above (which always carries the setup slot).
    let no_references: [vk::VideoReferenceSlotInfoKHR; 0] = [];
    let decode_info = vk::VideoDecodeInfoKHR::builder()
        .src_buffer(resources.bitstream_buffer)
        .src_buffer_offset(0)
        .src_buffer_range(params.bitstream_len)
        .dst_picture_resource(dst_resource)
        .setup_reference_slot(&setup_slot)
        .reference_slots(&no_references)
        .push_next(&mut av1_picture_info);
    // SAFETY: `decode_info` and everything it chains/borrows (picture info,
    // reference slots, dst/setup resources) stay alive for this call;
    // `command_buffer` is inside the active video-coding scope opened above.
    unsafe { device.cmd_decode_video_khr(command_buffer, &decode_info) };

    let end_info = vk::VideoEndCodingInfoKHR::builder();
    // SAFETY: ends the scope opened above on this same `command_buffer`.
    unsafe { device.cmd_end_video_coding_khr(command_buffer, &end_info) };

    // Restore this crate's fixed steady-state layout (`VIDEO_DECODE_DPB_KHR`)
    // on the slot just written — see `session_command_h264.rs`'s identical
    // barrier for the full reasoning.
    let from_decode_dst = vk::ImageMemoryBarrier2::builder()
        .src_stage_mask(vk::PipelineStageFlags2::VIDEO_DECODE_KHR)
        .src_access_mask(vk::AccessFlags2::VIDEO_DECODE_WRITE_KHR)
        .dst_stage_mask(all_commands)
        .dst_access_mask(memory_rw)
        .old_layout(vk::ImageLayout::VIDEO_DECODE_DST_KHR)
        .new_layout(vk::ImageLayout::VIDEO_DECODE_DPB_KHR)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(resources.dpb_image)
        .subresource_range(dst_range);
    let dep_info_back =
        vk::DependencyInfo::builder().image_memory_barriers(std::slice::from_ref(&from_decode_dst));
    // SAFETY: core Vulkan 1.3 call; `dep_info_back` and its barrier stay
    // alive for this call.
    unsafe { device.cmd_pipeline_barrier2(command_buffer, &dep_info_back) };

    // SAFETY: every command above was recorded into `command_buffer`, which
    // is currently in the recording state.
    unsafe { device.end_command_buffer(command_buffer) }.map_err(|result| {
        VulkanDecodeError::VkCall {
            call: "vkEndCommandBuffer",
            result,
        }
    })?;
    submit_and_wait(decode_device, resources, command_buffer)
}
