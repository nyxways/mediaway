//! `vkCmdCopyImageToBuffer` + host-visible mapped buffer, NV12 tight-packed,
//! for [`mediaway_decoder::VideoOutputPreference::CpuFramesOk`].
//!
//! Output layout matches `mediaway-decoder-linux`'s `vaapi/nv12.rs` and
//! `mediaway-decoder-windows`'s `wmf/cpu.rs` convention: `width * height`
//! luma bytes followed by `width * height / 2` interleaved chroma bytes,
//! tightly packed. Unlike those two (which re-pack a driver-chosen
//! stride/pitch out of an already-mapped image), this crate dictates the
//! destination layout directly via `VkBufferImageCopy`'s
//! `buffer_row_length`/`buffer_image_height` left at `0` — the Vulkan spec
//! defines `0` as "tightly packed per `image_extent`" — so no separate
//! re-pack pass is needed; the copy already produces the same tight layout.

#![allow(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Vulkan FFI: coded extent / buffer sizes are driver-bounded and small — casts \
              mirror mediaway-encoder-vulkan::session_command's identical allow."
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "workspace `unreachable_pub` policy (Cargo.toml) wants `pub(crate)` here; \
              clippy::pedantic's redundant_pub_crate disagrees for private modules — the \
              two lints are mutually exclusive for this shape, workspace policy wins"
)]
#![allow(
    clippy::too_many_lines,
    reason = "linear barrier -> copy -> barrier-back -> submit -> map sequence, mirrors \
              mediaway-encoder-vulkan::session_command's own record_and_submit shape"
)]

use vulkanalia::vk;
use vulkanalia::vk::{DeviceV1_0, DeviceV1_3, HasBuilder};

use crate::session::{DecodeDevice, VulkanDecodeError};
use crate::session_command::{SessionResources, color_range_for_layer};

/// Exact byte size of one tightly-packed NV12 frame (full-resolution luma
/// plane + half-resolution interleaved chroma) — matches
/// `mediaway-encoder-vulkan::session_encode::nv12_byte_size` exactly.
#[must_use]
pub(crate) const fn nv12_byte_size(width: u32, height: u32) -> vk::DeviceSize {
    let luma = (width as vk::DeviceSize) * (height as vk::DeviceSize);
    luma + luma / 2
}

/// Copies `resources.dpb_image`'s `slot_index` layer out to
/// `resources.readback_buffer` as tightly packed NV12, then maps and returns
/// the bytes.
///
/// Transitions the layer `VIDEO_DECODE_DPB_KHR` -> `TRANSFER_SRC_OPTIMAL` ->
/// back to `VIDEO_DECODE_DPB_KHR` around the copy — that layer must return to
/// `VIDEO_DECODE_DPB_KHR` since it may still be a live reference for later
/// pictures (see `session_command.rs`'s module doc on the DPB image's fixed
/// layout).
pub(crate) fn read_nv12(
    decode_device: &DecodeDevice<'_>,
    resources: &SessionResources,
    command_buffer: vk::CommandBuffer,
    coded_extent: vk::Extent2D,
    slot_index: u32,
) -> Result<Vec<u8>, VulkanDecodeError> {
    let device = decode_device.device;
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

    let all_commands = vk::PipelineStageFlags2::ALL_COMMANDS;
    let memory_rw = vk::AccessFlags2::MEMORY_READ | vk::AccessFlags2::MEMORY_WRITE;
    let range = color_range_for_layer(slot_index);
    let to_transfer_src = vk::ImageMemoryBarrier2::builder()
        .src_stage_mask(all_commands)
        .src_access_mask(memory_rw)
        .dst_stage_mask(all_commands)
        .dst_access_mask(memory_rw)
        .old_layout(vk::ImageLayout::VIDEO_DECODE_DPB_KHR)
        .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(resources.dpb_image)
        .subresource_range(range);
    let dep_info =
        vk::DependencyInfo::builder().image_memory_barriers(std::slice::from_ref(&to_transfer_src));
    // SAFETY: core Vulkan 1.3 call; `dep_info` and its barrier stay alive for
    // this call.
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
                base_array_layer: slot_index,
                layer_count: 1,
            })
            .image_extent(luma_extent),
        vk::BufferImageCopy::builder()
            .buffer_offset(luma_bytes)
            .image_subresource(vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::PLANE_1,
                mip_level: 0,
                base_array_layer: slot_index,
                layer_count: 1,
            })
            .image_extent(chroma_extent),
    ];
    // SAFETY: `resources.dpb_image`'s `slot_index` layer was just
    // transitioned to `TRANSFER_SRC_OPTIMAL` above; `resources.readback_buffer`
    // is sized `nv12_byte_size(coded_extent.width, coded_extent.height)`
    // (see `decoder.rs::VulkanVideoDecoder::open`), covering both regions'
    // combined range.
    unsafe {
        device.cmd_copy_image_to_buffer(
            command_buffer,
            resources.dpb_image,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            resources.readback_buffer,
            &copy_regions,
        );
    }

    let back_to_dpb = vk::ImageMemoryBarrier2::builder()
        .src_stage_mask(all_commands)
        .src_access_mask(memory_rw)
        .dst_stage_mask(all_commands)
        .dst_access_mask(memory_rw)
        .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
        .new_layout(vk::ImageLayout::VIDEO_DECODE_DPB_KHR)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(resources.dpb_image)
        .subresource_range(range);
    let dep_info_back =
        vk::DependencyInfo::builder().image_memory_barriers(std::slice::from_ref(&back_to_dpb));
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
    crate::session_command::submit_and_wait(decode_device, resources, command_buffer)?;

    let size = nv12_byte_size(coded_extent.width, coded_extent.height);
    // SAFETY: `resources.readback_memory` is bound to `resources.readback_buffer`,
    // sized `resources.readback_size >= size`; the fence wait inside
    // `submit_and_wait` above guarantees the GPU's writes to it are complete;
    // nothing else has it mapped.
    let ptr = unsafe {
        device.map_memory(
            resources.readback_memory,
            0,
            size,
            vk::MemoryMapFlags::empty(),
        )
    }
    .map_err(|result| VulkanDecodeError::VkCall {
        call: "vkMapMemory (readback)",
        result,
    })?;
    // SAFETY: `ptr` is valid for `size` bytes, just mapped above; memory type
    // is `HOST_COHERENT` so its contents are visible now that the fence has
    // signaled.
    let bytes = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), size as usize) }.to_vec();
    // SAFETY: `resources.readback_memory` was mapped by this same function
    // immediately above.
    unsafe { device.unmap_memory(resources.readback_memory) };
    Ok(bytes)
}

#[cfg(test)]
#[path = "cpu_readback_tests.rs"]
mod tests;
