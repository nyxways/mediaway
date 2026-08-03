//! Shared per-frame `vkCmdDecodeVideoKHR` recording + barriers (codec-generic
//! half — H.264-specific picture-info `pNext` construction lives in
//! `session_command_h264.rs`, HEVC's in `session_command_hevc.rs`).
//!
//! Owns [`SessionResources`]: every handle a [`crate::vulkan::decoder::VulkanVideoDecoder`]
//! allocates on top of its logical device and keeps alive across
//! `push_packet`/`poll_frame` calls (mirrors
//! `mediaway-encoder-vulkan::session_encode::SessionResources`'s identical
//! reusable-session shape).
//!
//! DPB image layout: **one** combined DPB-and-output image
//! (`DPB_AND_OUTPUT_COINCIDE`, see `session.rs`'s `query_capabilities`), with
//! one array layer per DPB slot. Every slot's layer is transitioned to
//! `VIDEO_DECODE_DPB_KHR` once at session-open time and never transitioned
//! again — the same layout serves both "this slot is a reference" and "this
//! slot is the current decode target" (the coincide capability's whole
//! point).

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

use vulkanalia::vk;
use vulkanalia::vk::{DeviceV1_0, DeviceV1_3, HasBuilder, KhrVideoQueueExtensionDeviceCommands};

use crate::vulkan::session::{DecodeDevice, VulkanDecodeError};

/// Every handle [`crate::vulkan::decoder::VulkanVideoDecoder`] allocates on top of the
/// logical device. All fields default to the Vulkan null handle
/// (`vkDestroy*`/`vkFree*` on a null handle is a documented no-op).
#[derive(Default)]
pub(crate) struct SessionResources {
    pub(crate) command_pool: vk::CommandPool,
    pub(crate) session: vk::VideoSessionKHR,
    pub(crate) session_memories: Vec<vk::DeviceMemory>,
    pub(crate) session_parameters: vk::VideoSessionParametersKHR,
    /// One combined DPB+output image, `dpb_slot_count` array layers.
    pub(crate) dpb_image: vk::Image,
    pub(crate) dpb_image_memory: vk::DeviceMemory,
    /// One shared `2D_ARRAY` `COLOR`-aspect view covering every layer — used
    /// as `dst_picture_resource`/`setup_reference_slot`'s/every reference
    /// slot's `image_view_binding`, with `base_array_layer` selecting the DPB
    /// slot within it (see `session_command.rs::create_dpb_image`'s doc).
    pub(crate) dpb_image_view: vk::ImageView,
    /// Host-visible buffer holding the current packet's Annex-B slice NAL
    /// bytes (start codes/emulation-prevention already stripped) for
    /// `VkVideoDecodeInfoKHR::src_buffer`.
    pub(crate) bitstream_buffer: vk::Buffer,
    pub(crate) bitstream_memory: vk::DeviceMemory,
    pub(crate) bitstream_capacity: vk::DeviceSize,
    /// Host-visible buffer `vkCmdCopyImageToBuffer` reads the decoded picture
    /// into, for [`crate::vulkan::cpu_readback`].
    pub(crate) readback_buffer: vk::Buffer,
    pub(crate) readback_memory: vk::DeviceMemory,
    pub(crate) readback_size: vk::DeviceSize,
    pub(crate) fence: vk::Fence,
    /// Whether `vkCmdControlVideoCodingKHR` with `RESET` has run yet (must
    /// happen once, inside its own begin/end video-coding scope, before the
    /// first `vkCmdDecodeVideoKHR` — ITU/Khronos Vulkan Video session
    /// lifecycle requirement).
    pub(crate) session_reset: bool,
}

impl SessionResources {
    /// Explicit best-effort teardown (mirrors
    /// `mediaway-encoder-vulkan::session_encode::SessionResources::destroy` —
    /// not a `Drop` impl, called once by `decoder.rs` after the fence wait
    /// guarantees no GPU work is outstanding).
    pub(crate) fn destroy(&self, device: &vulkanalia::Device) {
        // SAFETY: every handle here was either created by this same `device`
        // earlier in `VulkanVideoDecoder::open`, or is the default-initialized
        // null handle (a documented no-op for every `vkDestroy*`/`vkFree*`
        // call below). Called once, after the last fence wait, so nothing
        // here is still in use by the GPU.
        unsafe {
            device.destroy_fence(self.fence, None);
            device.destroy_buffer(self.readback_buffer, None);
            device.free_memory(self.readback_memory, None);
            device.destroy_buffer(self.bitstream_buffer, None);
            device.free_memory(self.bitstream_memory, None);
            device.destroy_image_view(self.dpb_image_view, None);
            device.destroy_image(self.dpb_image, None);
            device.free_memory(self.dpb_image_memory, None);
            device.destroy_video_session_parameters_khr(self.session_parameters, None);
            for &memory in &self.session_memories {
                device.free_memory(memory, None);
            }
            device.destroy_video_session_khr(self.session, None);
            device.destroy_command_pool(self.command_pool, None);
        }
    }
}

pub(crate) fn create_command_pool(
    device: &vulkanalia::Device,
    queue_family_index: u32,
) -> Result<vk::CommandPool, VulkanDecodeError> {
    let create_info = vk::CommandPoolCreateInfo::builder()
        .queue_family_index(queue_family_index)
        .flags(
            vk::CommandPoolCreateFlags::TRANSIENT
                | vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
        );
    // SAFETY: `create_info` valid; no allocator callbacks supplied.
    unsafe { device.create_command_pool(&create_info, None) }.map_err(|result| {
        VulkanDecodeError::VkCall {
            call: "vkCreateCommandPool",
            result,
        }
    })
}

pub(crate) fn allocate_command_buffer(
    device: &vulkanalia::Device,
    pool: vk::CommandPool,
) -> Result<vk::CommandBuffer, VulkanDecodeError> {
    let alloc_info = vk::CommandBufferAllocateInfo::builder()
        .command_pool(pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    // SAFETY: `pool` was just created on this `device`; `alloc_info` is valid.
    let buffers = unsafe { device.allocate_command_buffers(&alloc_info) }.map_err(|result| {
        VulkanDecodeError::VkCall {
            call: "vkAllocateCommandBuffers",
            result,
        }
    })?;
    buffers.into_iter().next().ok_or(VulkanDecodeError::VkCall {
        call: "vkAllocateCommandBuffers",
        result: vk::ErrorCode::UNKNOWN,
    })
}

pub(crate) fn create_fence(device: &vulkanalia::Device) -> Result<vk::Fence, VulkanDecodeError> {
    let create_info = vk::FenceCreateInfo::builder();
    // SAFETY: `create_info` valid; no allocator callbacks supplied.
    unsafe { device.create_fence(&create_info, None) }.map_err(|result| VulkanDecodeError::VkCall {
        call: "vkCreateFence",
        result,
    })
}

pub(crate) fn upload_to_host_memory(
    device: &vulkanalia::Device,
    memory: vk::DeviceMemory,
    data: &[u8],
) -> Result<(), VulkanDecodeError> {
    if data.is_empty() {
        return Ok(());
    }
    // SAFETY: `memory` was allocated and bound to a same-or-larger-sized
    // buffer by the caller; nothing else has it mapped.
    let ptr = unsafe {
        device.map_memory(
            memory,
            0,
            data.len() as vk::DeviceSize,
            vk::MemoryMapFlags::empty(),
        )
    }
    .map_err(|result| VulkanDecodeError::VkCall {
        call: "vkMapMemory",
        result,
    })?;
    // SAFETY: `ptr` is valid for `data.len()` bytes (just mapped above); the
    // two regions (`data` and the mapped range) do not overlap.
    unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), ptr.cast::<u8>(), data.len()) };
    // SAFETY: `memory` was mapped by this same function immediately above; the
    // memory type is `HOST_COHERENT` so no explicit flush is required.
    unsafe { device.unmap_memory(memory) };
    Ok(())
}

pub(crate) fn create_host_buffer(
    device: &vulkanalia::Device,
    memory_properties: &vk::PhysicalDeviceMemoryProperties,
    size: vk::DeviceSize,
    usage: vk::BufferUsageFlags,
) -> Result<(vk::Buffer, vk::DeviceMemory), VulkanDecodeError> {
    let create_info = vk::BufferCreateInfo::builder()
        .size(size)
        .usage(usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    // SAFETY: `create_info` valid; no allocator callbacks supplied.
    let buffer = unsafe { device.create_buffer(&create_info, None) }.map_err(|result| {
        VulkanDecodeError::VkCall {
            call: "vkCreateBuffer",
            result,
        }
    })?;
    // SAFETY: `buffer` was just created on this `device`.
    let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
    let type_index = crate::vulkan::session::find_memory_type(
        memory_properties,
        requirements.memory_type_bits,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    )?;
    let alloc_info = vk::MemoryAllocateInfo::builder()
        .allocation_size(requirements.size)
        .memory_type_index(type_index);
    // SAFETY: `alloc_info` valid; no allocator callbacks supplied.
    let memory = unsafe { device.allocate_memory(&alloc_info, None) }.map_err(|result| {
        VulkanDecodeError::VkCall {
            call: "vkAllocateMemory (buffer)",
            result,
        }
    })?;
    // SAFETY: `buffer`/`memory` both just created on this `device`, not yet
    // bound to anything else; `memory`'s size covers `requirements.size`.
    unsafe { device.bind_buffer_memory(buffer, memory, 0) }.map_err(|result| {
        VulkanDecodeError::VkCall {
            call: "vkBindBufferMemory",
            result,
        }
    })?;
    Ok((buffer, memory))
}

/// A whole-image `COLOR`-aspect subresource range for one array layer.
pub(crate) const fn color_range_for_layer(layer: u32) -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: layer,
        layer_count: 1,
    }
}

/// Creates the combined DPB+output image (`dpb_slot_count` array layers) and
/// transitions every layer to `VIDEO_DECODE_DPB_KHR` once — see the module
/// doc.
///
/// Returns a **single** `2D_ARRAY` image view covering every layer (not one
/// view per layer) — `VkVideoPictureResourceInfoKHR::baseArrayLayer`
/// (`session_command_h264.rs`/`session_command_hevc.rs`) selects the DPB slot
/// layer *within* this one shared view, matching the common
/// real-implementation pattern (a
/// single-layer view combined with a nonzero `baseArrayLayer` again was
/// tried first and found empirically to decode "successfully" per every
/// `VkResult` while producing all-zero output — no validation layer is
/// installed on this workspace's reference machine to have caught that
/// mismatch directly).
pub(crate) fn create_dpb_image(
    decode_device: &DecodeDevice<'_>,
    memory_properties: &vk::PhysicalDeviceMemoryProperties,
    profile: &mut crate::vulkan::session::DecodeProfile,
    format: vk::Format,
    coded_extent: vk::Extent2D,
    dpb_slot_count: u32,
) -> Result<(vk::Image, vk::DeviceMemory, vk::ImageView), VulkanDecodeError> {
    let device = decode_device.device;
    let profile_info = profile.info();
    let mut profile_list = vk::VideoProfileListInfoKHR::builder()
        .profiles(std::slice::from_ref(&profile_info))
        .build();
    let create_info = vk::ImageCreateInfo::builder()
        .flags(vk::ImageCreateFlags::MUTABLE_FORMAT | vk::ImageCreateFlags::EXTENDED_USAGE)
        .image_type(vk::ImageType::_2D)
        .format(format)
        .extent(vk::Extent3D {
            width: coded_extent.width,
            height: coded_extent.height,
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(dpb_slot_count)
        .samples(vk::SampleCountFlags::_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(
            vk::ImageUsageFlags::VIDEO_DECODE_DPB_KHR
                | vk::ImageUsageFlags::VIDEO_DECODE_DST_KHR
                | vk::ImageUsageFlags::TRANSFER_SRC,
        )
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .push_next(&mut profile_list);
    // SAFETY: `create_info` and its chained `profile_list`/`profile_info`
    // stay alive for this call; no allocator callbacks supplied.
    let image = unsafe { device.create_image(&create_info, None) }.map_err(|result| {
        VulkanDecodeError::VkCall {
            call: "vkCreateImage",
            result,
        }
    })?;
    // SAFETY: `image` was just created on this `device`.
    let requirements = unsafe { device.get_image_memory_requirements(image) };
    let type_index = crate::vulkan::session::find_memory_type(
        memory_properties,
        requirements.memory_type_bits,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    )?;
    let alloc_info = vk::MemoryAllocateInfo::builder()
        .allocation_size(requirements.size)
        .memory_type_index(type_index);
    // SAFETY: `alloc_info` valid; no allocator callbacks supplied.
    let memory = unsafe { device.allocate_memory(&alloc_info, None) }.map_err(|result| {
        VulkanDecodeError::VkCall {
            call: "vkAllocateMemory (image)",
            result,
        }
    })?;
    // SAFETY: `image`/`memory` both just created on this `device`, not yet
    // bound to anything else; `memory`'s size covers `requirements.size`.
    unsafe { device.bind_image_memory(image, memory, 0) }.map_err(|result| {
        VulkanDecodeError::VkCall {
            call: "vkBindImageMemory",
            result,
        }
    })?;

    let array_range = vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: dpb_slot_count,
    };
    let view_info = vk::ImageViewCreateInfo::builder()
        .image(image)
        .view_type(vk::ImageViewType::_2D_ARRAY)
        .format(format)
        .subresource_range(array_range);
    // SAFETY: `image` is bound to memory above; `view_info` is valid.
    let view = unsafe { device.create_image_view(&view_info, None) }.map_err(|result| {
        VulkanDecodeError::VkCall {
            call: "vkCreateImageView",
            result,
        }
    })?;
    Ok((image, memory, view))
}

/// Submits `command_buffer` and waits for it to finish (via
/// `resources.fence`, reset/reused across calls) — codec-generic, shared by
/// `session_command_h264.rs` and `session_command_hevc.rs`.
pub(crate) fn submit_and_wait(
    decode_device: &DecodeDevice<'_>,
    resources: &SessionResources,
    command_buffer: vk::CommandBuffer,
) -> Result<(), VulkanDecodeError> {
    let device = decode_device.device;
    // SAFETY: `resources.fence` was created once by the caller and is not
    // currently in use by any pending GPU work — the previous call's
    // `wait_for_fences` (if any) already guaranteed that.
    unsafe { device.reset_fences(&[resources.fence]) }.map_err(|result| {
        VulkanDecodeError::VkCall {
            call: "vkResetFences",
            result,
        }
    })?;
    let cb_submit_info = vk::CommandBufferSubmitInfo::builder()
        .command_buffer(command_buffer)
        .device_mask(0);
    let submit_info =
        vk::SubmitInfo2::builder().command_buffer_infos(std::slice::from_ref(&cb_submit_info));
    // SAFETY: `command_buffer` finished recording; `resources.fence` was just
    // reset, unsignaled.
    unsafe { device.queue_submit2(decode_device.queue, &[submit_info], resources.fence) }.map_err(
        |result| VulkanDecodeError::VkCall {
            call: "vkQueueSubmit2",
            result,
        },
    )?;
    // SAFETY: `resources.fence` was submitted with the queue submission
    // above.
    unsafe { device.wait_for_fences(&[resources.fence], true, u64::MAX) }.map_err(|result| {
        VulkanDecodeError::VkCall {
            call: "vkWaitForFences",
            result,
        }
    })?;
    Ok(())
}

/// Transitions every array layer of `resources.dpb_image` to
/// `VIDEO_DECODE_DPB_KHR` — called once at session-open time (see the module
/// doc: this layout is never changed again). Codec-generic, shared by both
/// H.264 and HEVC session construction.
pub(crate) fn transition_dpb_image_once(
    decode_device: &DecodeDevice<'_>,
    resources: &SessionResources,
    dpb_slot_count: u32,
    command_buffer: vk::CommandBuffer,
) -> Result<(), VulkanDecodeError> {
    let device = decode_device.device;
    // SAFETY: freshly allocated command buffer, never recorded.
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
    let barriers: Vec<vk::ImageMemoryBarrier2> = (0..dpb_slot_count)
        .map(|layer| {
            vk::ImageMemoryBarrier2::builder()
                .src_stage_mask(all_commands)
                .src_access_mask(vk::AccessFlags2::empty())
                .dst_stage_mask(all_commands)
                .dst_access_mask(memory_rw)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::VIDEO_DECODE_DPB_KHR)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(resources.dpb_image)
                .subresource_range(color_range_for_layer(layer))
                .build()
        })
        .collect();
    let dep_info = vk::DependencyInfo::builder().image_memory_barriers(&barriers);
    // SAFETY: core Vulkan 1.3 call; `dep_info` and its referenced barrier
    // array stay alive for this call.
    unsafe { device.cmd_pipeline_barrier2(command_buffer, &dep_info) };

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
