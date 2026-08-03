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
use vulkanalia::vk::{
    DeviceV1_0, DeviceV1_3, HasBuilder, KhrVideoEncodeQueueExtensionDeviceCommands,
    KhrVideoQueueExtensionDeviceCommands,
};

use crate::vulkan::session::{EncodeDevice, SessionResources, VulkanEncodeSessionError};

/// Grouped params for [`record_and_submit`] — keeps that function's own
/// signature under `clippy::too_many_arguments`.
pub(crate) struct RecordParams<'a> {
    pub(crate) command_buffer: vk::CommandBuffer,
    pub(crate) coded_extent: vk::Extent2D,
    pub(crate) dst_size: vk::DeviceSize,
    pub(crate) picture_info_pnext: &'a mut vk::VideoEncodeH264PictureInfoKHR,
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
/// barriers are deliberately coarse.
pub(crate) fn record_upload_and_barriers(
    device: &vulkanalia::Device,
    resources: &SessionResources,
    command_buffer: vk::CommandBuffer,
    coded_extent: vk::Extent2D,
    dst_size: vk::DeviceSize,
) {
    record_upload(device, resources, command_buffer, coded_extent);
    // SAFETY: `resources.dst_buffer` was just created with `TRANSFER_DST`
    // usage; this is the first command touching it, so no prior barrier is
    // needed before this write.
    unsafe { device.cmd_fill_buffer(command_buffer, resources.dst_buffer, 0, dst_size, 0) };
    record_pre_encode_barriers(device, resources, command_buffer, dst_size);
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
fn record_pre_encode_barriers(
    device: &vulkanalia::Device,
    resources: &SessionResources,
    command_buffer: vk::CommandBuffer,
    dst_size: vk::DeviceSize,
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
    let dpb_to_encode = vk::ImageMemoryBarrier2::builder()
        .src_stage_mask(all_commands)
        .src_access_mask(vk::AccessFlags2::empty())
        .dst_stage_mask(all_commands)
        .dst_access_mask(memory_rw)
        .old_layout(vk::ImageLayout::UNDEFINED)
        .new_layout(vk::ImageLayout::VIDEO_ENCODE_DPB_KHR)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(resources.dpb_image)
        .subresource_range(whole_color_range);
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
/// `vkCmdEndVideoCodingKHR` for the one synthetic IDR frame.
fn record_video_coding(
    encode_device: &EncodeDevice<'_>,
    resources: &SessionResources,
    params: &mut RecordParams<'_>,
    command_buffer: vk::CommandBuffer,
) {
    let device = encode_device.device;
    let dpb_resource = vk::VideoPictureResourceInfoKHR::builder()
        .coded_offset(vk::Offset2D { x: 0, y: 0 })
        .coded_extent(params.coded_extent)
        .base_array_layer(0)
        .image_view_binding(resources.dpb_image_view);
    let setup_slot = vk::VideoReferenceSlotInfoKHR::builder()
        .slot_index(0)
        .picture_resource(&dpb_resource);
    let begin_slots = [vk::VideoReferenceSlotInfoKHR::builder().slot_index(0)];

    // SAFETY: `resources.encode_feedback_query_pool` has exactly 1 query slot
    // (`create_encode_feedback_query_pool`); resetting before use is required
    // — a query pool's slots start in an undefined state.
    unsafe {
        device.cmd_reset_query_pool(command_buffer, resources.encode_feedback_query_pool, 0, 1);
    }
    let mut rate_control = vk::VideoEncodeRateControlInfoKHR::builder()
        .rate_control_mode(vk::VideoEncodeRateControlModeFlagsKHR::DISABLED);
    let begin_info = vk::VideoBeginCodingInfoKHR::builder()
        .video_session(resources.session)
        .video_session_parameters(resources.session_parameters)
        .reference_slots(&begin_slots)
        .push_next(&mut rate_control);
    // SAFETY: `begin_info` and its chained `rate_control`/`begin_slots` stay
    // alive for this call; `command_buffer` is currently recording.
    unsafe {
        device.cmd_begin_video_coding_khr(command_buffer, &begin_info);
    }

    let src_resource = vk::VideoPictureResourceInfoKHR::builder()
        .coded_offset(vk::Offset2D { x: 0, y: 0 })
        .coded_extent(params.coded_extent)
        .base_array_layer(0)
        .image_view_binding(resources.input_image_view);
    let encode_info = vk::VideoEncodeInfoKHR::builder()
        .dst_buffer(resources.dst_buffer)
        .dst_buffer_offset(0)
        .dst_buffer_range(params.dst_size)
        .src_picture_resource(src_resource)
        .setup_reference_slot(&setup_slot)
        .push_next(params.picture_info_pnext);
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
    // (`src_resource`/`setup_slot`/`dpb_resource`/the H.264 picture-info
    // chain built by the caller) stay alive for this call; `command_buffer`
    // is inside an active video-coding scope from the `cmd_begin_video_coding_khr`
    // call immediately above.
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
