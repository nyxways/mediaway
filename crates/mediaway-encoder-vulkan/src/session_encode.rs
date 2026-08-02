//! Stage 1 continued: video session parameters + DPB/input images + staging
//! upload + command recording/submission/readback, and the crate's public
//! entry point, [`encode_synthetic_intra_frame`]. Split from `session.rs`
//! only to respect this workspace's 1000-line-per-source-file rule — both
//! files are one logical unit; `session.rs`'s module doc records the real
//! scope and the cuts this stage made.

#![allow(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "Vulkan FFI: every count/size here is driver-reported and small \
              (single digits to low thousands — queue families, DPB slots, \
              memory requirement counts, one coded picture's byte size); casts \
              mirror the generated builder code's own conventions (e.g. `.len() as _`)."
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
    DeviceV1_0, HasBuilder, InstanceV1_0, KhrVideoEncodeQueueExtensionDeviceCommands,
    KhrVideoQueueExtensionDeviceCommands,
};

use crate::h264_params::{self, McAlignedExtent};
use crate::session::{
    Capabilities, DeviceGuard, EncodeDevice, EncodeProfile, EncodedFrame, InstanceGuard,
    SessionResources, VulkanEncodeSessionError, create_instance, create_logical_device,
    find_h264_encode_device, find_memory_type, query_capabilities, query_video_format,
};
use crate::session_command::{RecordParams, record_and_submit};

/// Runs the whole Stage 1 pipeline once on real hardware.
///
/// instance → device → video session → session parameters → DPB/input
/// images → one `vkCmdEncodeVideoKHR` submission → bitstream readback. See
/// `session.rs`'s module doc for exactly what is (and isn't) covered.
///
/// # Errors
/// Returns [`VulkanEncodeSessionError`] at the first failing Vulkan call —
/// see that enum's `VkCall` variant for which call and `VkResult`.
pub fn encode_synthetic_intra_frame() -> Result<EncodedFrame, VulkanEncodeSessionError> {
    let (_entry, instance_guard) = create_instance()?;
    let InstanceGuard { instance } = &instance_guard;
    let (physical_device, queue_family_index) = find_h264_encode_device(instance)?;

    let mut profile = EncodeProfile::new_h264();
    let capabilities = query_capabilities(instance, physical_device, &mut profile)?;
    let coded_extent = capabilities.min_coded_extent;
    let mc_extent = McAlignedExtent::from_pixels(coded_extent.width, coded_extent.height).ok_or(
        VulkanEncodeSessionError::DegenerateCodedExtent {
            width: coded_extent.width,
            height: coded_extent.height,
        },
    )?;
    let input_format = query_video_format(
        instance,
        physical_device,
        &mut profile,
        vk::ImageUsageFlags::VIDEO_ENCODE_SRC_KHR,
    )?;
    let dpb_format = query_video_format(
        instance,
        physical_device,
        &mut profile,
        vk::ImageUsageFlags::VIDEO_ENCODE_DPB_KHR,
    )?;

    let device_guard = create_logical_device(instance, physical_device, queue_family_index)?;
    let DeviceGuard { device } = &device_guard;
    // SAFETY: `queue_family_index`/index `0` were the exact queue this
    // `device` was created with in `create_logical_device`.
    let queue = unsafe { device.get_device_queue(queue_family_index, 0) };
    let encode_device = EncodeDevice {
        device,
        queue,
        queue_family_index,
    };
    // SAFETY: `physical_device` came from `find_h264_encode_device` on this
    // same `instance`.
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
    )?;
    resources.session = session;
    resources.session_memories = session_memories;

    let sps = h264_params::build_sps(mc_extent);
    let pps = h264_params::build_pps();
    resources.session_parameters = create_session_parameters(&encode_device, session, &sps, &pps)?;
    let header_bytes = get_encoded_headers(&encode_device, resources.session_parameters)?;

    let dst_size = create_images_and_buffers(
        &encode_device,
        &memory_properties,
        &mut profile,
        input_format,
        dpb_format,
        coded_extent,
        capabilities.min_bitstream_buffer_size_alignment,
        &mut resources,
    )?;
    upload_to_host_memory(
        device,
        resources.staging_memory,
        &synthetic_gray_nv12(coded_extent.width, coded_extent.height),
    )?;

    let ref_lists = h264_params::build_empty_reference_lists();
    let mut picture_info = h264_params::build_idr_picture_info();
    picture_info.pRefLists = &raw const ref_lists;
    let slice_header = h264_params::build_idr_slice_header();
    let nalu_slice_entries = [vk::VideoEncodeH264NaluSliceInfoKHR::builder()
        .constant_qp(26)
        .std_slice_header(&slice_header)
        .build()];
    let mut h264_picture_info = vk::VideoEncodeH264PictureInfoKHR::builder()
        .nalu_slice_entries(&nalu_slice_entries)
        .std_picture_info(&picture_info)
        .generate_prefix_nalu(false)
        .build();

    resources.command_pool = create_command_pool(device, queue_family_index)?;
    let command_buffer = allocate_command_buffer(device, resources.command_pool)?;
    resources.fence = create_fence(device)?;
    resources.encode_feedback_query_pool = create_encode_feedback_query_pool(device, &mut profile)?;

    let mut record_params = RecordParams {
        command_buffer,
        coded_extent,
        dst_size,
        picture_info_pnext: &mut h264_picture_info,
    };
    let dst_bytes = record_and_submit(&encode_device, &mut resources, &mut record_params)?;

    resources.destroy(device);

    let mut bitstream = header_bytes;
    bitstream.extend_from_slice(&dst_bytes);
    Ok(EncodedFrame {
        bitstream,
        coded_width: coded_extent.width,
        coded_height: coded_extent.height,
    })
}

/// One IDR-baseline video session sized for `coded_extent`, plus every
/// `VkDeviceMemory` `vkGetVideoSessionMemoryRequirementsKHR` asked for
/// (device-local — codec-internal state, never mapped by the host).
#[allow(
    clippy::too_many_arguments,
    reason = "internal helper, called once, clearer un-bundled"
)]
pub(crate) fn create_video_session(
    encode_device: &EncodeDevice<'_>,
    memory_properties: &vk::PhysicalDeviceMemoryProperties,
    profile: &mut EncodeProfile,
    capabilities: &Capabilities,
    coded_extent: vk::Extent2D,
    picture_format: vk::Format,
    reference_format: vk::Format,
) -> Result<(vk::VideoSessionKHR, Vec<vk::DeviceMemory>), VulkanEncodeSessionError> {
    let device = encode_device.device;
    let profile_info = profile.info();
    let create_info = vk::VideoSessionCreateInfoKHR::builder()
        .queue_family_index(encode_device.queue_family_index)
        .video_profile(&profile_info)
        .picture_format(picture_format)
        .max_coded_extent(coded_extent)
        .reference_picture_format(reference_format)
        .max_dpb_slots(1)
        .max_active_reference_pictures(0)
        .std_header_version(&capabilities.std_header_version);
    // SAFETY: `create_info` and everything it chains/borrows are alive for
    // this single synchronous call; no allocator callbacks supplied.
    let session =
        unsafe { device.create_video_session_khr(&create_info, None) }.map_err(|result| {
            VulkanEncodeSessionError::VkCall {
                call: "vkCreateVideoSessionKHR",
                result,
            }
        })?;

    // SAFETY: `session` was just created on this `device`.
    let reqs =
        unsafe { device.get_video_session_memory_requirements_khr(session) }.map_err(|result| {
            VulkanEncodeSessionError::VkCall {
                call: "vkGetVideoSessionMemoryRequirementsKHR",
                result,
            }
        })?;

    let mut memories = Vec::with_capacity(reqs.len());
    for req in &reqs {
        // No specific property requirement here (unlike the DPB/input images
        // below): a `VkVideoSessionKHR`'s per-bind-index memory requirements
        // are opaque driver-internal state. Hardware-verified 2026-07-29 that
        // requiring `DEVICE_LOCAL` is *wrong* here — on this RTX 4090, one
        // bind index reports `memoryTypeBits = 0x8`, which on this driver
        // matches only memory type 3: `HOST_VISIBLE | HOST_COHERENT |
        // HOST_CACHED` on the non-device-local heap (`vulkaninfo`). Accept
        // whatever memory type the driver's own bitmask allows instead of
        // second-guessing it.
        let type_index = find_memory_type(
            memory_properties,
            req.memory_requirements.memory_type_bits,
            vk::MemoryPropertyFlags::empty(),
        )?;
        let alloc_info = vk::MemoryAllocateInfo::builder()
            .allocation_size(req.memory_requirements.size)
            .memory_type_index(type_index);
        // SAFETY: `alloc_info` valid; no allocator callbacks supplied.
        let memory = unsafe { device.allocate_memory(&alloc_info, None) }.map_err(|result| {
            VulkanEncodeSessionError::VkCall {
                call: "vkAllocateMemory (video session)",
                result,
            }
        })?;
        memories.push(memory);
    }
    let binds: Vec<vk::BindVideoSessionMemoryInfoKHR> = reqs
        .iter()
        .zip(memories.iter())
        .map(|(req, &memory)| {
            vk::BindVideoSessionMemoryInfoKHR::builder()
                .memory_bind_index(req.memory_bind_index)
                .memory(memory)
                .memory_offset(0)
                .memory_size(req.memory_requirements.size)
                .build()
        })
        .collect();
    // SAFETY: `binds` has one entry per successfully allocated memory above;
    // `session`/`memories` are all still alive.
    unsafe { device.bind_video_session_memory_khr(session, &binds) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkBindVideoSessionMemoryKHR",
            result,
        }
    })?;

    Ok((session, memories))
}

pub(crate) fn create_session_parameters(
    encode_device: &EncodeDevice<'_>,
    session: vk::VideoSessionKHR,
    sps: &native::StdVideoH264SequenceParameterSet,
    pps: &native::StdVideoH264PictureParameterSet,
) -> Result<vk::VideoSessionParametersKHR, VulkanEncodeSessionError> {
    let device = encode_device.device;
    let add_info = vk::VideoEncodeH264SessionParametersAddInfoKHR::builder()
        .std_sp_ss(std::slice::from_ref(sps))
        .std_pp_ss(std::slice::from_ref(pps));
    let mut h264_create_info = vk::VideoEncodeH264SessionParametersCreateInfoKHR::builder()
        .max_std_sps_count(1)
        .max_std_pps_count(1)
        .parameters_add_info(&add_info)
        .build();
    let create_info = vk::VideoSessionParametersCreateInfoKHR::builder()
        .video_session(session)
        .push_next(&mut h264_create_info);
    // SAFETY: `create_info` and its chained `add_info`/`sps`/`pps` stay alive
    // for this single synchronous call; no allocator callbacks supplied.
    unsafe { device.create_video_session_parameters_khr(&create_info, None) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkCreateVideoSessionParametersKHR",
            result,
        }
    })
}

/// HEVC sibling of [`create_session_parameters`] — a third parameter set
/// (VPS) alongside SPS/PPS, otherwise identical shape.
pub(crate) fn create_session_parameters_hevc(
    encode_device: &EncodeDevice<'_>,
    session: vk::VideoSessionKHR,
    vps: &native::StdVideoH265VideoParameterSet,
    sps: &native::StdVideoH265SequenceParameterSet,
    pps: &native::StdVideoH265PictureParameterSet,
) -> Result<vk::VideoSessionParametersKHR, VulkanEncodeSessionError> {
    let device = encode_device.device;
    let add_info = vk::VideoEncodeH265SessionParametersAddInfoKHR::builder()
        .std_vp_ss(std::slice::from_ref(vps))
        .std_sp_ss(std::slice::from_ref(sps))
        .std_pp_ss(std::slice::from_ref(pps));
    let mut hevc_create_info = vk::VideoEncodeH265SessionParametersCreateInfoKHR::builder()
        .max_std_vps_count(1)
        .max_std_sps_count(1)
        .max_std_pps_count(1)
        .parameters_add_info(&add_info)
        .build();
    let create_info = vk::VideoSessionParametersCreateInfoKHR::builder()
        .video_session(session)
        .push_next(&mut hevc_create_info);
    // SAFETY: `create_info` and its chained `add_info`/`vps`/`sps`/`pps` stay
    // alive for this single synchronous call; no allocator callbacks supplied.
    unsafe { device.create_video_session_parameters_khr(&create_info, None) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkCreateVideoSessionParametersKHR",
            result,
        }
    })
}

/// AV1 sibling of [`create_session_parameters`]/[`create_session_parameters_hevc`]
/// — one sequence header + one operating point, no "add info" list (AV1 has
/// no multiple-SPS/PPS-set concept the way H.264/HEVC do). No
/// `get_encoded_headers`-style AV1 sibling exists — see `av1_params.rs`'s
/// module doc for why.
pub(crate) fn create_session_parameters_av1(
    encode_device: &EncodeDevice<'_>,
    session: vk::VideoSessionKHR,
    sequence_header: &native::StdVideoAV1SequenceHeader,
    operating_points: &[native::StdVideoEncodeAV1OperatingPointInfo],
) -> Result<vk::VideoSessionParametersKHR, VulkanEncodeSessionError> {
    let device = encode_device.device;
    let mut av1_create_info = vk::VideoEncodeAV1SessionParametersCreateInfoKHR::builder()
        .std_sequence_header(sequence_header)
        .std_operating_points(operating_points)
        .build();
    let create_info = vk::VideoSessionParametersCreateInfoKHR::builder()
        .video_session(session)
        .push_next(&mut av1_create_info);
    // SAFETY: `create_info` and its chained `av1_create_info`/`sequence_header`/
    // `operating_points` stay alive for this single synchronous call; no
    // allocator callbacks supplied.
    unsafe { device.create_video_session_parameters_khr(&create_info, None) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkCreateVideoSessionParametersKHR",
            result,
        }
    })
}

/// Fetches Annex-B `SPS`+`PPS` NAL bytes matching `session_parameters`
/// exactly, via `vkGetEncodedVideoSessionParametersKHR`.
pub(crate) fn get_encoded_headers(
    encode_device: &EncodeDevice<'_>,
    session_parameters: vk::VideoSessionParametersKHR,
) -> Result<Vec<u8>, VulkanEncodeSessionError> {
    let device = encode_device.device;
    let mut h264_get_info = vk::VideoEncodeH264SessionParametersGetInfoKHR::builder()
        .write_std_sps(true)
        .write_std_pps(true)
        .std_sps_id(0)
        .std_pps_id(0)
        .build();
    let get_info = vk::VideoEncodeSessionParametersGetInfoKHR::builder()
        .video_session_parameters(session_parameters)
        .push_next(&mut h264_get_info);
    // SAFETY: `get_info` and its chained `h264_get_info` stay alive for this
    // call; the trait wrapper performs the standard two-call size/fill pattern
    // internally.
    unsafe { device.get_encoded_video_session_parameters_khr(&get_info, None) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkGetEncodedVideoSessionParametersKHR",
            result,
        }
    })
}

/// AV1 sibling of [`get_encoded_headers`] — fetches the `OBU_SEQUENCE_HEADER`
/// OBU bytes matching `session_parameters` exactly. Unlike H.264/HEVC, no
/// codec-specific `pNext` struct is chained: the Vulkan registry defines no
/// `VkVideoEncodeAV1SessionParametersGetInfoKHR` at all, because an AV1
/// session parameters object stores exactly **one** sequence header (no
/// SPS/PPS-id list to select from — see `av1_params.rs`'s module doc and the
/// `VK_KHR_video_encode_av1` proposal doc's
/// "`vkGetEncodedVideoSessionParametersKHR`... filled with the encoded
/// bitstream of the requested AV1 sequence header" note), so the base
/// `VkVideoEncodeSessionParametersGetInfoKHR::videoSessionParameters` handle
/// alone is enough to identify what to fetch.
///
/// Also reports `VkVideoEncodeSessionParametersFeedbackInfoKHR::hasOverrides`
/// — whether the driver silently changed any app-supplied sequence-header
/// field to conform to hardware limits. `FFmpeg`'s real, hardware-tested
/// `vulkan_encode_av1.c` always checks this and, when set, re-derives its
/// sequence header from these exact returned bytes before recreating session
/// parameters — see `encoder.rs`'s AV1 branch for whether/how this crate
/// acts on it.
pub(crate) fn get_encoded_headers_av1(
    encode_device: &EncodeDevice<'_>,
    session_parameters: vk::VideoSessionParametersKHR,
) -> Result<(Vec<u8>, bool), VulkanEncodeSessionError> {
    let device = encode_device.device;
    let get_info = vk::VideoEncodeSessionParametersGetInfoKHR::builder()
        .video_session_parameters(session_parameters);
    let mut feedback = vk::VideoEncodeSessionParametersFeedbackInfoKHR::builder().build();
    // SAFETY: `get_info` stays alive for this call; `feedback` is a valid
    // out-param the driver writes into; the trait wrapper performs the
    // standard two-call size/fill pattern internally.
    let bytes =
        unsafe { device.get_encoded_video_session_parameters_khr(&get_info, Some(&mut feedback)) }
            .map_err(|result| VulkanEncodeSessionError::VkCall {
                call: "vkGetEncodedVideoSessionParametersKHR",
                result,
            })?;
    Ok((bytes, feedback.has_overrides != 0))
}

/// HEVC sibling of [`get_encoded_headers`] — fetches Annex-B `VPS`+`SPS`+`PPS`
/// NAL bytes.
pub(crate) fn get_encoded_headers_hevc(
    encode_device: &EncodeDevice<'_>,
    session_parameters: vk::VideoSessionParametersKHR,
) -> Result<Vec<u8>, VulkanEncodeSessionError> {
    let device = encode_device.device;
    let mut hevc_get_info = vk::VideoEncodeH265SessionParametersGetInfoKHR::builder()
        .write_std_vps(true)
        .write_std_sps(true)
        .write_std_pps(true)
        .std_vps_id(0)
        .std_sps_id(0)
        .std_pps_id(0)
        .build();
    let get_info = vk::VideoEncodeSessionParametersGetInfoKHR::builder()
        .video_session_parameters(session_parameters)
        .push_next(&mut hevc_get_info);
    // SAFETY: `get_info` and its chained `hevc_get_info` stay alive for this
    // call; the trait wrapper performs the standard two-call size/fill pattern
    // internally.
    unsafe { device.get_encoded_video_session_parameters_khr(&get_info, None) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkGetEncodedVideoSessionParametersKHR",
            result,
        }
    })
}

/// Creates the input (encode-source) image, the DPB image, uploads a
/// synthetic gray `NV12` frame into a staging buffer, and creates the
/// zero-length-checked destination bitstream buffer — filling in every image
/// and buffer field of `resources`. Returns the destination buffer's size
/// (the only one of these values the caller still needs afterward).
#[allow(
    clippy::too_many_arguments,
    reason = "internal helper, called once, clearer un-bundled"
)]
pub(crate) fn create_images_and_buffers(
    encode_device: &EncodeDevice<'_>,
    memory_properties: &vk::PhysicalDeviceMemoryProperties,
    profile: &mut EncodeProfile,
    input_format: vk::Format,
    dpb_format: vk::Format,
    coded_extent: vk::Extent2D,
    bitstream_alignment: vk::DeviceSize,
    resources: &mut SessionResources,
) -> Result<vk::DeviceSize, VulkanEncodeSessionError> {
    let device = encode_device.device;
    let (input_image, input_image_memory, input_image_view) = create_video_image(
        encode_device,
        memory_properties,
        profile,
        input_format,
        coded_extent,
        vk::ImageUsageFlags::VIDEO_ENCODE_SRC_KHR,
    )?;
    resources.input_image = input_image;
    resources.input_image_memory = input_image_memory;
    resources.input_image_view = input_image_view;

    let (dpb_image, dpb_image_memory, dpb_image_view) = create_video_image(
        encode_device,
        memory_properties,
        profile,
        dpb_format,
        coded_extent,
        vk::ImageUsageFlags::VIDEO_ENCODE_DPB_KHR,
    )?;
    resources.dpb_image = dpb_image;
    resources.dpb_image_memory = dpb_image_memory;
    resources.dpb_image_view = dpb_image_view;

    // Staging buffer sized for one NV12 frame at `coded_extent`, left
    // unwritten here — the caller uploads real per-frame bytes via
    // [`upload_to_host_memory`] on every push, not just once (see
    // `encoder.rs::VulkanVideoEncoder::push_frame`).
    let staging_size = nv12_byte_size(coded_extent.width, coded_extent.height);
    let (staging_buffer, staging_memory) = create_host_buffer(
        device,
        memory_properties,
        staging_size,
        vk::BufferUsageFlags::TRANSFER_SRC,
    )?;
    resources.staging_buffer = staging_buffer;
    resources.staging_memory = staging_memory;

    let dst_size = bitstream_buffer_size(coded_extent, bitstream_alignment);
    let (dst_buffer, dst_memory) = create_host_buffer(
        device,
        memory_properties,
        dst_size,
        vk::BufferUsageFlags::VIDEO_ENCODE_DST_KHR | vk::BufferUsageFlags::TRANSFER_DST,
    )?;
    resources.dst_buffer = dst_buffer;
    resources.dst_memory = dst_memory;

    Ok(dst_size)
}

/// Exact byte size of one tightly-packed NV12 frame (`G8_B8R8_2PLANE_420_UNORM`
/// layout: full-resolution luma plane + half-resolution interleaved chroma).
pub(crate) const fn nv12_byte_size(width: u32, height: u32) -> vk::DeviceSize {
    let luma = (width as vk::DeviceSize) * (height as vk::DeviceSize);
    luma + luma / 2
}

/// Worst-case compressed bitstream size for one coded picture at `extent`,
/// aligned up to the driver's `min_bitstream_buffer_size_alignment` — mirrors
/// `mediaway-encoder-windows`'s D3D12 backend's `BITSTREAM_SAFETY_MARGIN`
/// formula (`width*height*3` plus headroom), generalized here from the
/// original single-shot path's hardcoded `4096` (only ever exercised at one
/// small resolution).
fn bitstream_buffer_size(extent: vk::Extent2D, alignment: vk::DeviceSize) -> vk::DeviceSize {
    const SAFETY_MARGIN: vk::DeviceSize = 65_536;
    let raw = (vk::DeviceSize::from(extent.width) * vk::DeviceSize::from(extent.height)) * 3
        + SAFETY_MARGIN;
    let align = alignment.max(1);
    raw.div_ceil(align) * align
}

/// One `VK_KHR_video_encode_queue`-usable image (encode source or DPB): a
/// device-local `VkImage` created inside `profile`'s profile list, its
/// backing memory, and a whole-image `COLOR`-aspect view (this crate never
/// sets `DISJOINT`, so per spec a combined-plane `COLOR` view is correct —
/// see `session.rs`'s module doc).
fn create_video_image(
    encode_device: &EncodeDevice<'_>,
    memory_properties: &vk::PhysicalDeviceMemoryProperties,
    profile: &mut EncodeProfile,
    format: vk::Format,
    extent: vk::Extent2D,
    usage: vk::ImageUsageFlags,
) -> Result<(vk::Image, vk::DeviceMemory, vk::ImageView), VulkanEncodeSessionError> {
    let device = encode_device.device;
    let profile_info = profile.info();
    let mut profile_list = vk::VideoProfileListInfoKHR::builder()
        .profiles(std::slice::from_ref(&profile_info))
        .build();
    let create_info = vk::ImageCreateInfo::builder()
        .flags(vk::ImageCreateFlags::MUTABLE_FORMAT | vk::ImageCreateFlags::EXTENDED_USAGE)
        .image_type(vk::ImageType::_2D)
        .format(format)
        .extent(vk::Extent3D {
            width: extent.width,
            height: extent.height,
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .push_next(&mut profile_list);
    // SAFETY: `create_info` and its chained `profile_list`/`profile_info`
    // stay alive for this call; no allocator callbacks supplied.
    let image = unsafe { device.create_image(&create_info, None) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkCreateImage",
            result,
        }
    })?;
    // SAFETY: `image` was just created on this `device`.
    let requirements = unsafe { device.get_image_memory_requirements(image) };
    let type_index = find_memory_type(
        memory_properties,
        requirements.memory_type_bits,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    )?;
    let alloc_info = vk::MemoryAllocateInfo::builder()
        .allocation_size(requirements.size)
        .memory_type_index(type_index);
    // SAFETY: `alloc_info` valid; no allocator callbacks supplied.
    let memory = unsafe { device.allocate_memory(&alloc_info, None) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkAllocateMemory (image)",
            result,
        }
    })?;
    // SAFETY: `image`/`memory` both just created on this `device`, not yet
    // bound to anything else; `memory`'s size covers `requirements.size`.
    unsafe { device.bind_image_memory(image, memory, 0) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkBindImageMemory",
            result,
        }
    })?;
    let view_info = vk::ImageViewCreateInfo::builder()
        .image(image)
        .view_type(vk::ImageViewType::_2D)
        .format(format)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        });
    // SAFETY: `image` is bound to memory above; `view_info` is valid.
    let view = unsafe { device.create_image_view(&view_info, None) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkCreateImageView",
            result,
        }
    })?;
    Ok((image, memory, view))
}

/// A solid mid-gray NV12-equivalent (`G8_B8R8_2PLANE_420_UNORM`) frame: luma
/// plane filled `128`, chroma plane filled `128,128` (colour-neutral) — the
/// "synthetic all-intra frame" the task asked for, deliberately as simple as
/// possible.
fn synthetic_gray_nv12(width: u32, height: u32) -> Vec<u8> {
    let luma_len = (width * height) as usize;
    let chroma_len = luma_len / 2;
    let mut data = vec![128u8; luma_len + chroma_len];
    data.truncate(luma_len + chroma_len);
    data
}

fn create_host_buffer(
    device: &vulkanalia::Device,
    memory_properties: &vk::PhysicalDeviceMemoryProperties,
    size: vk::DeviceSize,
    usage: vk::BufferUsageFlags,
) -> Result<(vk::Buffer, vk::DeviceMemory), VulkanEncodeSessionError> {
    let create_info = vk::BufferCreateInfo::builder()
        .size(size)
        .usage(usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    // SAFETY: `create_info` valid; no allocator callbacks supplied.
    let buffer = unsafe { device.create_buffer(&create_info, None) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkCreateBuffer",
            result,
        }
    })?;
    // SAFETY: `buffer` was just created on this `device`.
    let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
    let type_index = find_memory_type(
        memory_properties,
        requirements.memory_type_bits,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    )?;
    let alloc_info = vk::MemoryAllocateInfo::builder()
        .allocation_size(requirements.size)
        .memory_type_index(type_index);
    // SAFETY: `alloc_info` valid; no allocator callbacks supplied.
    let memory = unsafe { device.allocate_memory(&alloc_info, None) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkAllocateMemory (buffer)",
            result,
        }
    })?;
    // SAFETY: `buffer`/`memory` both just created on this `device`, not yet
    // bound to anything else; `memory`'s size covers `requirements.size`.
    unsafe { device.bind_buffer_memory(buffer, memory, 0) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkBindBufferMemory",
            result,
        }
    })?;
    Ok((buffer, memory))
}

pub(crate) fn upload_to_host_memory(
    device: &vulkanalia::Device,
    memory: vk::DeviceMemory,
    data: &[u8],
) -> Result<(), VulkanEncodeSessionError> {
    // SAFETY: `memory` was just allocated and bound to a same-sized buffer by
    // `create_host_buffer`; nothing else has it mapped.
    let ptr = unsafe {
        device.map_memory(
            memory,
            0,
            data.len() as vk::DeviceSize,
            vk::MemoryMapFlags::empty(),
        )
    }
    .map_err(|result| VulkanEncodeSessionError::VkCall {
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

/// `RESET_COMMAND_BUFFER` (not just `TRANSIENT`) so
/// [`crate::encoder::VulkanVideoEncoder::push_frame`] can `vkResetCommandBuffer`
/// and re-record the same buffer every frame instead of allocating a fresh
/// one — [`encode_synthetic_intra_frame`] itself only records once, so this
/// flag is unused there, but harmless (a reset that's never called).
pub(crate) fn create_command_pool(
    device: &vulkanalia::Device,
    queue_family_index: u32,
) -> Result<vk::CommandPool, VulkanEncodeSessionError> {
    let create_info = vk::CommandPoolCreateInfo::builder()
        .queue_family_index(queue_family_index)
        .flags(
            vk::CommandPoolCreateFlags::TRANSIENT
                | vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
        );
    // SAFETY: `create_info` valid; no allocator callbacks supplied.
    unsafe { device.create_command_pool(&create_info, None) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkCreateCommandPool",
            result,
        }
    })
}

/// One-slot `VK_QUERY_TYPE_VIDEO_ENCODE_FEEDBACK_KHR` query pool requesting
/// only `BITSTREAM_BYTES_WRITTEN` — lets [`crate::session_command::record_video_coding`]
/// bracket `vkCmdEncodeVideoKHR` with `vkCmdBeginQuery`/`vkCmdEndQuery` so the
/// driver reports the real per-frame compressed byte count (read back via
/// `vkGetQueryPoolResults` after the fence wait), instead of this crate
/// returning the whole zero-padded destination buffer on every packet — the
/// [`encode_synthetic_intra_frame`] Stage 1 diagnostic's original scope cut
/// (see `session.rs`'s module doc "Scope cuts"), now closed for the real
/// [`crate::encoder::VulkanVideoEncoder`].
pub(crate) fn create_encode_feedback_query_pool(
    device: &vulkanalia::Device,
    profile: &mut EncodeProfile,
) -> Result<vk::QueryPool, VulkanEncodeSessionError> {
    let mut feedback_info = vk::QueryPoolVideoEncodeFeedbackCreateInfoKHR::builder()
        .encode_feedback_flags(vk::VideoEncodeFeedbackFlagsKHR::BITSTREAM_BYTES_WRITTEN)
        .build();
    let mut profile_info = profile.info();
    let create_info = vk::QueryPoolCreateInfo::builder()
        .query_type(vk::QueryType::VIDEO_ENCODE_FEEDBACK_KHR)
        .query_count(1)
        .push_next(&mut feedback_info)
        .push_next(&mut profile_info);
    // SAFETY: `create_info` and its chained `feedback_info`/`profile_info`
    // (and the codec profile struct that chains onto, in turn) stay alive for
    // this single synchronous call; no allocator callbacks supplied.
    unsafe { device.create_query_pool(&create_info, None) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkCreateQueryPool",
            result,
        }
    })
}

/// Creates one unsignaled fence, reused (via `vkResetFences`) across every
/// [`crate::session_command::record_and_submit`] call on the same session —
/// see [`crate::encoder::VulkanVideoEncoder`].
pub(crate) fn create_fence(
    device: &vulkanalia::Device,
) -> Result<vk::Fence, VulkanEncodeSessionError> {
    let create_info = vk::FenceCreateInfo::builder();
    // SAFETY: `create_info` valid; no allocator callbacks supplied.
    unsafe { device.create_fence(&create_info, None) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkCreateFence",
            result,
        }
    })
}

pub(crate) fn allocate_command_buffer(
    device: &vulkanalia::Device,
    pool: vk::CommandPool,
) -> Result<vk::CommandBuffer, VulkanEncodeSessionError> {
    let alloc_info = vk::CommandBufferAllocateInfo::builder()
        .command_pool(pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    // SAFETY: `pool` was just created on this `device`; `alloc_info` is valid.
    let buffers = unsafe { device.allocate_command_buffers(&alloc_info) }.map_err(|result| {
        VulkanEncodeSessionError::VkCall {
            call: "vkAllocateCommandBuffers",
            result,
        }
    })?;
    buffers
        .into_iter()
        .next()
        .ok_or(VulkanEncodeSessionError::VkCall {
            call: "vkAllocateCommandBuffers",
            result: vk::ErrorCode::UNKNOWN,
        })
}
