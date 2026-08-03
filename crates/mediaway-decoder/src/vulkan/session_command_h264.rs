//! H.264-specific `vkCmdDecodeVideoKHR` picture-info `pNext` payload
//! construction, plus the H.264 `record_and_submit_h264` entry point built on
//! `session_command.rs`'s shared [`SessionResources`].

#![allow(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "Vulkan FFI: every count/size here is driver-reported and small — casts mirror \
              mediaway-encoder-vulkan::session_command's identical allow."
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
              decode -> end video coding -> submit), mirrors \
              mediaway-encoder-vulkan::session_command's own record_and_submit shape"
)]

use vulkanalia::vk;
use vulkanalia::vk::video as native;
use vulkanalia::vk::{
    DeviceV1_0, DeviceV1_3, HasBuilder, KhrVideoDecodeQueueExtensionDeviceCommands,
    KhrVideoQueueExtensionDeviceCommands,
};

use crate::vulkan::dpb::DpbSlot;
use crate::vulkan::h264_params::reference_info_from_slot;
use crate::vulkan::session::{DecodeDevice, VulkanDecodeError};
use crate::vulkan::session_command::{SessionResources, color_range_for_layer, submit_and_wait};

/// One `StdVideoDecodeH264PictureInfo` this crate builds per decoded picture.
#[must_use]
pub(crate) fn build_picture_info(
    seq_parameter_set_id: u8,
    pic_parameter_set_id: u8,
    frame_num: u16,
    pic_order_cnt: i32,
    is_idr: bool,
    is_reference: bool,
    is_intra: bool,
) -> native::StdVideoDecodeH264PictureInfo {
    let mut flags = native::StdVideoDecodeH264PictureInfoFlags {
        _bitfield_align_1: [],
        _bitfield_1: native::__BindgenBitfieldUnit::new([0u8; 1]),
        __bindgen_padding_0: [0; 3],
    };
    flags.set_IdrPicFlag(u32::from(is_idr));
    flags.set_is_reference(u32::from(is_reference));
    flags.set_is_intra(u32::from(is_intra));
    native::StdVideoDecodeH264PictureInfo {
        flags,
        seq_parameter_set_id,
        pic_parameter_set_id,
        reserved1: 0,
        reserved2: 0,
        frame_num,
        idr_pic_id: 0,
        PicOrderCnt: [pic_order_cnt, pic_order_cnt],
    }
}

/// Everything [`record_and_submit_h264`] needs for one picture's decode
/// command. Grouped to stay under `clippy::too_many_arguments`.
pub(crate) struct RecordParamsH264<'a> {
    pub(crate) command_buffer: vk::CommandBuffer,
    pub(crate) coded_extent: vk::Extent2D,
    /// Bytes of `resources.bitstream_buffer` actually holding this picture's
    /// Annex-B slice NAL payload (start code / emulation-prevention already
    /// stripped) — the rest of the buffer's capacity is unused this frame.
    pub(crate) bitstream_len: vk::DeviceSize,
    /// DPB array-layer index this picture decodes into.
    pub(crate) dst_slot_index: u32,
    /// Other DPB slots this picture may reference (excludes `dst_slot_index`).
    pub(crate) reference_slots: &'a [(u32, DpbSlot)],
    pub(crate) picture_info: &'a native::StdVideoDecodeH264PictureInfo,
}

/// Runs the one-time `vkCmdControlVideoCodingKHR` `RESET`, inside its own
/// begin/end video-coding scope, required before the first
/// `vkCmdDecodeVideoKHR` on a freshly created session.
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

/// Records and submits one picture's `vkCmdDecodeVideoKHR`, waits for
/// completion, then leaves the decoded picture in `resources.dpb_image`'s
/// `params.dst_slot_index` layer for the caller to read back
/// ([`crate::vulkan::cpu_readback`]) or hand out as a Zero-Copy handle
/// ([`crate::vulkan::zero_copy`]).
pub(crate) fn record_and_submit_h264(
    decode_device: &DecodeDevice<'_>,
    resources: &mut SessionResources,
    params: &RecordParamsH264<'_>,
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

    // Host-write-to-device-read barrier for the bitstream buffer (coarse
    // ALL_COMMANDS/MEMORY_READ|WRITE, matching mediaway-encoder-vulkan's own
    // "one-shot correctness, not perf-tuned sync" barrier convention).
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
    // The destination slot's layer must be `VIDEO_DECODE_DST_KHR` (not
    // `VIDEO_DECODE_DPB_KHR`) for the duration of *this* decode command, even
    // in this crate's coincide/layered-DPB design (one shared image, one
    // shared view) — a genuine, previously-missed requirement, confirmed
    // against a real working implementation (FFmpeg's `vulkan_decode.c`:
    // `(layered_dpb || vp->dpb_frame) ? VIDEO_DECODE_DST_KHR :
    // VIDEO_DECODE_DPB_KHR`). Every other slot (already-decoded references)
    // stays in `VIDEO_DECODE_DPB_KHR`, this crate's fixed steady-state layout
    // (see `session_command.rs::create_dpb_image`'s doc) — restored on this
    // slot too immediately after `vkCmdEndVideoCodingKHR` below, so
    // `cpu_readback.rs`'s "every slot starts in `VIDEO_DECODE_DPB_KHR`"
    // assumption keeps holding for the next picture.
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

    // Build the current picture's and every reference slot's
    // VkVideoReferenceSlotInfoKHR (kept alive on this stack frame as named
    // bindings — not temporaries — for the whole recording below).
    // `base_array_layer` selects the DPB slot layer within the one shared
    // `2D_ARRAY` view (see `session_command.rs::create_dpb_image`'s doc).
    let dst_resource = vk::VideoPictureResourceInfoKHR::builder()
        .coded_offset(vk::Offset2D { x: 0, y: 0 })
        .coded_extent(params.coded_extent)
        .base_array_layer(params.dst_slot_index)
        .image_view_binding(resources.dpb_image_view)
        .build();
    let dst_reference_info = reference_info_from_slot(&DpbSlot {
        frame_num: u32::from(params.picture_info.frame_num),
        frame_num_wrap: i32::from(params.picture_info.frame_num),
        pic_order_cnt: params.picture_info.PicOrderCnt[0],
        used_for_reference: params.picture_info.flags.is_reference() != 0,
    });
    let mut dst_dpb_slot_info = vk::VideoDecodeH264DpbSlotInfoKHR::builder()
        .std_reference_info(&dst_reference_info)
        .build();
    let setup_slot = vk::VideoReferenceSlotInfoKHR::builder()
        .slot_index(params.dst_slot_index as i32)
        .picture_resource(&dst_resource)
        .push_next(&mut dst_dpb_slot_info)
        .build();

    let ref_resources: Vec<vk::VideoPictureResourceInfoKHR> = params
        .reference_slots
        .iter()
        .map(|(slot_index, _)| {
            vk::VideoPictureResourceInfoKHR::builder()
                .coded_offset(vk::Offset2D { x: 0, y: 0 })
                .coded_extent(params.coded_extent)
                .base_array_layer(*slot_index)
                .image_view_binding(resources.dpb_image_view)
                .build()
        })
        .collect();
    let ref_reference_infos: Vec<native::StdVideoDecodeH264ReferenceInfo> = params
        .reference_slots
        .iter()
        .map(|(_, slot)| reference_info_from_slot(slot))
        .collect();
    let mut ref_dpb_slot_infos: Vec<vk::VideoDecodeH264DpbSlotInfoKHR> = ref_reference_infos
        .iter()
        .map(|info| {
            vk::VideoDecodeH264DpbSlotInfoKHR::builder()
                .std_reference_info(info)
                .build()
        })
        .collect();
    let reference_slots: Vec<vk::VideoReferenceSlotInfoKHR> = params
        .reference_slots
        .iter()
        .zip(ref_resources.iter())
        .zip(ref_dpb_slot_infos.iter_mut())
        .map(|(((slot_index, _), resource), dpb_slot_info)| {
            vk::VideoReferenceSlotInfoKHR::builder()
                .slot_index(*slot_index as i32)
                .picture_resource(resource)
                .push_next(dpb_slot_info)
                .build()
        })
        .collect();

    // vkCmdBeginVideoCodingKHR: every established reference slot, plus the
    // slot about to be written — but that slot is introduced with
    // `slotIndex = -1`, **not** its real index, since it is not yet an
    // active/established reference this scope (only `pSetupReferenceSlot` in
    // the `VkVideoDecodeInfoKHR` below carries the real index). This exact
    // real-index-vs-`-1` distinction is a genuine, load-bearing Vulkan Video
    // protocol requirement this crate initially missed — real-index in both
    // places decoded with no `VkResult` error but wrote no observable output
    // (confirmed via FFmpeg's own working `libavcodec/vulkan_decode.c`:
    // `cur_vk_ref[0] = vp->ref_slot; cur_vk_ref[0].slotIndex = -1;` is exactly
    // this pattern — a full copy of the setup slot with only `slotIndex`
    // overridden).
    // clone: `reference_slots` is reused as-is for `VkVideoDecodeInfoKHR`'s
    // own `reference_slots` below; `begin_slots` needs the same entries plus
    // one extra (the `-1`-indexed setup slot copy), and both arrays must stay
    // alive independently as named locals for the two separate Vulkan calls
    // below.
    let mut begin_slots = reference_slots.clone();
    let mut begin_setup_slot = setup_slot;
    begin_setup_slot.slot_index = -1;
    begin_slots.push(begin_setup_slot);
    let begin_coding = vk::VideoBeginCodingInfoKHR::builder()
        .video_session(resources.session)
        .video_session_parameters(resources.session_parameters)
        .reference_slots(&begin_slots);
    // SAFETY: `begin_coding` and its referenced slot array stay alive for
    // this call; `command_buffer` is recording.
    unsafe { device.cmd_begin_video_coding_khr(command_buffer, &begin_coding) };

    let slice_offsets = [0u32];
    let mut h264_picture_info = vk::VideoDecodeH264PictureInfoKHR::builder()
        .std_picture_info(params.picture_info)
        .slice_offsets(&slice_offsets)
        .build();
    let decode_info = vk::VideoDecodeInfoKHR::builder()
        .src_buffer(resources.bitstream_buffer)
        .src_buffer_offset(0)
        .src_buffer_range(params.bitstream_len)
        .dst_picture_resource(dst_resource)
        .setup_reference_slot(&setup_slot)
        .reference_slots(&reference_slots)
        .push_next(&mut h264_picture_info);
    // SAFETY: `decode_info` and everything it chains/borrows (picture info,
    // reference slots, dst/setup resources) stay alive for this call;
    // `command_buffer` is inside the active video-coding scope opened above.
    unsafe { device.cmd_decode_video_khr(command_buffer, &decode_info) };

    let end_info = vk::VideoEndCodingInfoKHR::builder();
    // SAFETY: ends the scope opened above on this same `command_buffer`.
    unsafe { device.cmd_end_video_coding_khr(command_buffer, &end_info) };

    // Restore this crate's fixed steady-state layout (`VIDEO_DECODE_DPB_KHR`,
    // see `session_command.rs::create_dpb_image`'s doc) on the slot just
    // written, now that the video-coding scope has ended — so it is ready
    // either to serve as a reference for a later picture (its
    // `VkVideoReferenceSlotInfoKHR` entries above always assume
    // `VIDEO_DECODE_DPB_KHR`) or for `cpu_readback.rs`'s own
    // `VIDEO_DECODE_DPB_KHR` -> `TRANSFER_SRC_OPTIMAL` transition (recorded by
    // the caller after this returns, in the same command buffer — left to the
    // caller since not every `poll_frame` call needs a CPU copy; Zero-Copy
    // callers skip it).
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
