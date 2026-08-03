//! Vulkan instance/device/video-session plumbing shared by every codec this
//! crate decodes (H.264 and HEVC this round; AV1 is a follow-up).
//!
//! Mirrors `mediaway-encoder-vulkan::session`'s shape closely (same
//! instance/device/capabilities/session/session-parameters sequence, decode
//! flavored): [`DecodeProfile`] plays the same codec-generic-enum role as
//! that crate's `EncodeProfile`.

#![allow(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "Vulkan FFI: every count/size here is driver-reported and small (queue families, \
              DPB slots, memory requirement counts) — casts mirror vulkanalia's own generated \
              builder code and mediaway-encoder-vulkan::session's identical allow."
)]

use thiserror::Error;
use vulkanalia::vk;
use vulkanalia::vk::{
    DeviceV1_0, HasBuilder, InstanceV1_0, KhrVideoQueueExtensionDeviceCommands,
    KhrVideoQueueExtensionInstanceCommands,
};

use crate::vulkan::dpb::DpbError;
use crate::vulkan::h264_params::H264ParamError;

/// This crate's fixed per-codec decode profile.
///
/// H.264 and HEVC this round (AV1 remains a follow-up) — an enum (not a bare
/// struct) so an `Av1` variant can be added later without restructuring
/// callers (`session.rs`, `session_command.rs`, `decoder.rs`), matching
/// `adr/0001`'s sketch and `mediaway-encoder-vulkan::session::EncodeProfile`'s
/// identical shape.
pub enum DecodeProfile {
    /// H.264 decode profile.
    H264(vk::VideoDecodeH264ProfileInfoKHR),
    /// HEVC decode profile.
    Hevc(vk::VideoDecodeH265ProfileInfoKHR),
}

impl DecodeProfile {
    /// H.264 Baseline/Main/High-compatible profile — `std_profile_idc` is not
    /// fixed to one constant: real streams commonly signal Main (77) or High
    /// (100); the Vulkan decode profile only needs the *maximum* profile this
    /// session should accept, so this crate always requests High (100), a
    /// superset any Baseline/Main/High stream can be decoded under.
    #[must_use]
    pub fn new_h264() -> Self {
        Self::H264(
            vk::VideoDecodeH264ProfileInfoKHR::builder()
                .std_profile_idc(vulkanalia::vk::video::STD_VIDEO_H264_PROFILE_IDC_BASELINE)
                .picture_layout(vk::VideoDecodeH264PictureLayoutFlagsKHR::PROGRESSIVE)
                .build(),
        )
    }

    /// HEVC Main-profile decode profile — this crate's `hevc_params.rs`
    /// rejects anything beyond Main-profile syntax (no RExt/SCC extensions),
    /// so Main is both the request and the real ceiling, unlike H.264's
    /// "request a superset" choice above.
    #[must_use]
    pub fn new_hevc() -> Self {
        Self::Hevc(
            vk::VideoDecodeH265ProfileInfoKHR::builder()
                .std_profile_idc(vulkanalia::vk::video::STD_VIDEO_H265_PROFILE_IDC_MAIN)
                .build(),
        )
    }

    /// The `VkVideoProfileInfoKHR` this profile chains its codec-specific
    /// struct onto — borrows `self`, so `self` must outlive every use of the
    /// returned value (mirrors `EncodeProfile::info`'s identical contract).
    pub fn info(&mut self) -> vk::VideoProfileInfoKHR {
        let base = vk::VideoProfileInfoKHR::builder()
            .chroma_subsampling(vk::VideoChromaSubsamplingFlagsKHR::_420)
            .luma_bit_depth(vk::VideoComponentBitDepthFlagsKHR::_8)
            .chroma_bit_depth(vk::VideoComponentBitDepthFlagsKHR::_8);
        match self {
            Self::H264(h264) => base
                .video_codec_operation(vk::VideoCodecOperationFlagsKHR::DECODE_H264)
                .push_next(h264)
                .build(),
            Self::Hevc(hevc) => base
                .video_codec_operation(vk::VideoCodecOperationFlagsKHR::DECODE_H265)
                .push_next(hevc)
                .build(),
        }
    }
}

/// Failures from this crate's Vulkan Video decode plumbing.
///
/// Every FFI call site maps its `Result`/`VkResult` through this enum.
/// Crate-internal — see `decoder.rs`'s `map_err` for how this becomes
/// `crate::DecodeError` at the public boundary (`DecodeError`
/// itself gains no new variant, per `adr/0001`'s decision).
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum VulkanDecodeError {
    /// No usable Vulkan loader was found, or it could not be resolved via
    /// `libloading` at runtime.
    #[error("failed to load the Vulkan loader: {0}")]
    Loader(Box<dyn vulkanalia::loader::LoaderError>),
    /// `vkCreateInstance` failed.
    #[error("vkCreateInstance failed: {0:?}")]
    CreateInstance(vk::ErrorCode),
    /// `vkEnumeratePhysicalDevices` failed.
    #[error("vkEnumeratePhysicalDevices failed: {0:?}")]
    EnumeratePhysicalDevices(vk::ErrorCode),
    /// No physical device on this host advertises an H.264 decode queue
    /// family.
    #[error(
        "no physical device advertises a VK_VIDEO_CODEC_OPERATION_DECODE_H264_BIT_KHR queue family"
    )]
    NoDecodeCapableDevice,
    /// A raw Vulkan Video call returned a non-`SUCCESS` [`vk::Result`].
    #[error("{call} failed: {result:?}")]
    VkCall {
        /// The C function name that failed, e.g. `"vkCreateVideoSessionKHR"`.
        call: &'static str,
        /// The raw `VkResult` it returned.
        result: vk::ErrorCode,
    },
    /// `vkGetPhysicalDeviceVideoFormatPropertiesKHR` reported zero formats for
    /// the requested image usage under this profile.
    #[error("driver reported no VK_KHR_video_decode_queue image format for usage {usage:?}")]
    NoVideoFormat {
        /// The image usage flags that had no matching format.
        usage: vk::ImageUsageFlags,
    },
    /// The driver requires separate DPB and output images
    /// (`VK_VIDEO_DECODE_CAPABILITY_DPB_AND_OUTPUT_DISTINCT_BIT_KHR` only,
    /// not `_COINCIDE_`) — this crate's session/image allocation only
    /// implements the coincide case this round (see `session.rs`'s
    /// `create_images` doc).
    #[error("driver requires separate DPB/output images (DPB_AND_OUTPUT_COINCIDE not advertised)")]
    SeparateReferenceImagesRequired,
    /// The caller-requested width/height falls outside the driver's reported
    /// coded-extent bounds/alignment.
    #[error(
        "requested {width}x{height} outside driver-reported coded-extent bounds \
         {min_width}x{min_height}..={max_width}x{max_height} (granularity \
         {granularity_width}x{granularity_height})"
    )]
    UnsupportedResolution {
        /// Caller-requested width, in pixels.
        width: u32,
        /// Caller-requested height, in pixels.
        height: u32,
        /// Driver-reported minimum coded width.
        min_width: u32,
        /// Driver-reported minimum coded height.
        min_height: u32,
        /// Driver-reported maximum coded width.
        max_width: u32,
        /// Driver-reported maximum coded height.
        max_height: u32,
        /// Driver-reported `picture_access_granularity` width.
        granularity_width: u32,
        /// Driver-reported `picture_access_granularity` height.
        granularity_height: u32,
    },
    /// No entry in `VkPhysicalDeviceMemoryProperties::memoryTypes` satisfies
    /// both a resource's `memoryTypeBits` mask and the required property
    /// flags.
    #[error(
        "no memory type matches requirements (type_bits={type_bits:#x}, required={required:?})"
    )]
    NoMemoryType {
        /// `VkMemoryRequirements::memoryTypeBits` from the failing resource.
        type_bits: u32,
        /// The `VkMemoryPropertyFlags` this crate required.
        required: vk::MemoryPropertyFlags,
    },
    /// A parsed H.264 SPS/PPS or slice header used a syntax element this
    /// crate does not support (see `h264_params.rs`/`h264_slice.rs`), or the
    /// input bytes were truncated/malformed.
    #[error(transparent)]
    Bitstream(#[from] H264ParamError),
    /// A parsed HEVC VPS/SPS/PPS or slice-segment-header used a syntax
    /// element this crate does not support (see `hevc_params.rs`/
    /// `hevc_slice.rs`), or the input bytes were truncated/malformed.
    #[error(transparent)]
    HevcBitstream(#[from] crate::vulkan::hevc_params::HevcParamError),
    /// A DPB slot-bookkeeping operation failed (see `dpb.rs`).
    #[error(transparent)]
    Dpb(#[from] DpbError),
    /// A packet was pushed before an SPS/PPS had been seen, or referenced an
    /// unknown parameter-set id.
    #[error("no active SPS/PPS for this packet (id {id})")]
    MissingParameterSet {
        /// The `seq_parameter_set_id`/`pic_parameter_set_id` that was missing.
        id: u32,
    },
}

/// RAII guard for the throwaway/owned Vulkan instance.
pub(crate) struct InstanceGuard {
    pub(crate) instance: vulkanalia::Instance,
}
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        // SAFETY: sole owner of `self.instance`, created by this module and
        // never cloned or handed out; no allocation callbacks were supplied
        // at creation.
        unsafe { self.instance.destroy_instance(None) };
    }
}

/// RAII guard for the logical device.
pub(crate) struct DeviceGuard {
    pub(crate) device: vulkanalia::Device,
}
impl Drop for DeviceGuard {
    fn drop(&mut self) {
        // SAFETY: sole owner; every resource built on `self.device` must be
        // destroyed by the caller before this guard drops (see
        // `decoder.rs::VulkanVideoDecoder`'s explicit teardown before its own
        // field drop order runs).
        unsafe { self.device.destroy_device(None) };
    }
}

/// Bundles the device-derived handles decode helpers need, so none of them
/// individually trips `clippy::too_many_arguments`.
pub(crate) struct DecodeDevice<'a> {
    pub(crate) device: &'a vulkanalia::Device,
    pub(crate) queue: vk::Queue,
    pub(crate) queue_family_index: u32,
}

/// Creates a throwaway instance the same way `probe.rs` does (see that
/// module's comment on why `VK_KHR_video_queue` must not be requested as an
/// *instance* extension).
pub(crate) fn create_instance() -> Result<(vulkanalia::Entry, InstanceGuard), VulkanDecodeError> {
    // SAFETY: `LibloadingLoader` dynamically resolves the system Vulkan loader;
    // the resulting function pointers are only invoked through this `Entry`
    // and instances derived from it.
    let loader = unsafe { vulkanalia::loader::LibloadingLoader::new(vulkanalia::loader::LIBRARY) }
        .map_err(|error| VulkanDecodeError::Loader(error.into()))?;
    let entry = unsafe { vulkanalia::Entry::new(loader) }.map_err(VulkanDecodeError::Loader)?;
    let app_info = vk::ApplicationInfo::builder()
        .application_name(b"mediaway-decoder-vulkan-session\0")
        .api_version(vk::make_version(1, 3, 0));
    let create_info = vk::InstanceCreateInfo::builder().application_info(&app_info);
    // SAFETY: `create_info` borrows `app_info`, alive for this call; no
    // allocation callbacks supplied.
    let instance = unsafe { entry.create_instance(&create_info, None) }
        .map_err(VulkanDecodeError::CreateInstance)?;
    Ok((entry, InstanceGuard { instance }))
}

/// Finds the first physical device + queue family advertising `op` decode
/// (mirrors `probe::probe_one_device`'s query, duplicated rather than shared
/// — see `mediaway-encoder-vulkan::session`'s identical note: this module
/// intentionally does not depend on `probe`'s short-lived instance).
/// [`find_h264_decode_device`]/[`find_hevc_decode_device`] are thin
/// codec-specific wrappers, matching
/// `mediaway-encoder-vulkan::session::find_h264_encode_device`'s shape.
fn find_decode_device(
    instance: &vulkanalia::Instance,
    op: vk::VideoCodecOperationFlagsKHR,
) -> Result<(vk::PhysicalDevice, u32), VulkanDecodeError> {
    // SAFETY: `instance` is a live instance owned by the caller for the
    // duration of this call.
    let physical_devices = unsafe { instance.enumerate_physical_devices() }
        .map_err(VulkanDecodeError::EnumeratePhysicalDevices)?;

    for physical_device in physical_devices {
        // SAFETY: `physical_device` was just enumerated from `instance`.
        let family_count =
            unsafe { instance.get_physical_device_queue_family_properties(physical_device) }.len();
        let mut video_props: Vec<vk::QueueFamilyVideoPropertiesKHR> = (0..family_count)
            .map(|_| vk::QueueFamilyVideoPropertiesKHR::default())
            .collect();
        let mut families2: Vec<vk::QueueFamilyProperties2> = video_props
            .iter_mut()
            .map(|entry| {
                vk::QueueFamilyProperties2::builder()
                    .push_next(entry)
                    .build()
            })
            .collect();
        // SAFETY: `families2` has exactly `family_count` entries, each
        // chaining a live `QueueFamilyVideoPropertiesKHR` from `video_props`
        // (which outlives this call) for the driver to write into.
        unsafe {
            let mut written = u32::try_from(family_count).unwrap_or(u32::MAX);
            (instance
                .commands()
                .get_physical_device_queue_family_properties2)(
                physical_device,
                &raw mut written,
                families2.as_mut_ptr(),
            );
        }
        let found = video_props
            .iter()
            .position(|p| p.video_codec_operations.contains(op));
        if let Some(index) = found {
            let queue_family_index = u32::try_from(index).unwrap_or(u32::MAX);
            return Ok((physical_device, queue_family_index));
        }
    }
    Err(VulkanDecodeError::NoDecodeCapableDevice)
}

pub(crate) fn find_h264_decode_device(
    instance: &vulkanalia::Instance,
) -> Result<(vk::PhysicalDevice, u32), VulkanDecodeError> {
    find_decode_device(instance, vk::VideoCodecOperationFlagsKHR::DECODE_H264)
}

pub(crate) fn find_hevc_decode_device(
    instance: &vulkanalia::Instance,
) -> Result<(vk::PhysicalDevice, u32), VulkanDecodeError> {
    find_decode_device(instance, vk::VideoCodecOperationFlagsKHR::DECODE_H265)
}

/// Real capability numbers this crate needs from
/// `vkGetPhysicalDeviceVideoCapabilitiesKHR`.
pub(crate) struct Capabilities {
    pub(crate) min_coded_extent: vk::Extent2D,
    pub(crate) max_coded_extent: vk::Extent2D,
    pub(crate) picture_access_granularity: vk::Extent2D,
    pub(crate) max_dpb_slots: u32,
    pub(crate) max_active_reference_pictures: u32,
    pub(crate) std_header_version: vk::ExtensionProperties,
    /// Required alignment for `VkVideoDecodeInfoKHR::srcBufferRange` — a
    /// non-conformant (unaligned) range is real driver-observed cause of a
    /// silently no-op `vkCmdDecodeVideoKHR` (no `VkResult` failure at all,
    /// found empirically: no validation layer is installed on this
    /// workspace's reference machine to have caught the spec violation
    /// directly). See `decoder.rs`'s bitstream upload, which rounds the
    /// uploaded range up to this alignment.
    pub(crate) min_bitstream_buffer_size_alignment: vk::DeviceSize,
}

impl Capabilities {
    /// Validates a caller-requested coded extent against this driver's
    /// reported bounds and alignment.
    pub(crate) const fn validate_requested_extent(
        &self,
        width: u32,
        height: u32,
    ) -> Result<(), VulkanDecodeError> {
        let in_range = width >= self.min_coded_extent.width
            && width <= self.max_coded_extent.width
            && height >= self.min_coded_extent.height
            && height <= self.max_coded_extent.height;
        let aligned = self.picture_access_granularity.width != 0
            && self.picture_access_granularity.height != 0
            && width % self.picture_access_granularity.width == 0
            && height % self.picture_access_granularity.height == 0;
        if in_range && aligned {
            return Ok(());
        }
        Err(VulkanDecodeError::UnsupportedResolution {
            width,
            height,
            min_width: self.min_coded_extent.width,
            min_height: self.min_coded_extent.height,
            max_width: self.max_coded_extent.width,
            max_height: self.max_coded_extent.height,
            granularity_width: self.picture_access_granularity.width,
            granularity_height: self.picture_access_granularity.height,
        })
    }
}

/// Queries `vkGetPhysicalDeviceVideoCapabilitiesKHR`, requiring
/// `DPB_AND_OUTPUT_COINCIDE` (see [`VulkanDecodeError::SeparateReferenceImagesRequired`]'s
/// doc — this crate's session/image allocation only implements the coincide
/// case this round: one image array serves as both DPB storage and decode
/// output, avoiding a second image set + extra copy this round would not
/// otherwise need).
pub(crate) fn query_capabilities(
    instance: &vulkanalia::Instance,
    physical_device: vk::PhysicalDevice,
    profile: &mut DecodeProfile,
) -> Result<Capabilities, VulkanDecodeError> {
    // The codec-specific capabilities struct chained below **must** match
    // `profile`'s own codec — chaining H.264's capabilities struct while
    // querying an HEVC profile made a real driver reject the whole call
    // outright on `mediaway-encoder-vulkan`'s own reference RTX 4090 (see
    // that crate's `session.rs::query_capabilities`) — determined before
    // `profile.info()` mutably borrows `profile` for the rest of this
    // function.
    let is_hevc = matches!(profile, DecodeProfile::Hevc(_));
    let profile_info = profile.info();
    let mut h264_caps = vk::VideoDecodeH264CapabilitiesKHR::default();
    let mut hevc_caps = vk::VideoDecodeH265CapabilitiesKHR::default();
    let mut decode_caps = vk::VideoDecodeCapabilitiesKHR::default();
    let mut caps_builder = vk::VideoCapabilitiesKHR::builder().push_next(&mut decode_caps);
    caps_builder = if is_hevc {
        caps_builder.push_next(&mut hevc_caps)
    } else {
        caps_builder.push_next(&mut h264_caps)
    };
    let mut caps = caps_builder.build();
    // SAFETY: `profile_info`/`caps` (and their chained extension structs)
    // stay alive for this single synchronous call; `caps` is a valid
    // out-param destination the driver writes into.
    let result = unsafe {
        instance.get_physical_device_video_capabilities_khr(
            physical_device,
            &profile_info,
            &mut caps,
        )
    };
    result.map_err(|result| VulkanDecodeError::VkCall {
        call: "vkGetPhysicalDeviceVideoCapabilitiesKHR",
        result,
    })?;
    if !decode_caps
        .flags
        .contains(vk::VideoDecodeCapabilityFlagsKHR::DPB_AND_OUTPUT_COINCIDE)
    {
        return Err(VulkanDecodeError::SeparateReferenceImagesRequired);
    }
    Ok(Capabilities {
        min_coded_extent: caps.min_coded_extent,
        max_coded_extent: caps.max_coded_extent,
        picture_access_granularity: caps.picture_access_granularity,
        max_dpb_slots: caps.max_dpb_slots,
        max_active_reference_pictures: caps.max_active_reference_pictures,
        std_header_version: caps.std_header_version,
        min_bitstream_buffer_size_alignment: caps.min_bitstream_buffer_size_alignment,
    })
}

/// Queries the concrete [`vk::Format`] the driver wants for a combined
/// decode-output-and-DPB image (`DPB_AND_OUTPUT_COINCIDE`, see
/// [`query_capabilities`]) — 2-call pattern
/// (`vkGetPhysicalDeviceVideoFormatPropertiesKHR`), first entry only.
pub(crate) fn query_video_format(
    instance: &vulkanalia::Instance,
    physical_device: vk::PhysicalDevice,
    profile: &mut DecodeProfile,
    usage: vk::ImageUsageFlags,
) -> Result<vk::Format, VulkanDecodeError> {
    let profile_info = profile.info();
    let mut profile_list = vk::VideoProfileListInfoKHR::builder()
        .profiles(std::slice::from_ref(&profile_info))
        .build();
    let format_info = vk::PhysicalDeviceVideoFormatInfoKHR::builder()
        .image_usage(usage)
        .push_next(&mut profile_list)
        .build();
    // SAFETY: `format_info` (and its chained `profile_list`, in turn chaining
    // `profile_info`) stays alive for this call; the trait wrapper performs
    // the standard two-call array-enumeration pattern internally.
    let formats = unsafe {
        instance.get_physical_device_video_format_properties_khr(physical_device, &format_info)
    }
    .map_err(|result| VulkanDecodeError::VkCall {
        call: "vkGetPhysicalDeviceVideoFormatPropertiesKHR",
        result,
    })?;
    formats
        .first()
        .map(|f| f.format)
        .ok_or(VulkanDecodeError::NoVideoFormat { usage })
}

pub(crate) fn create_logical_device(
    instance: &vulkanalia::Instance,
    physical_device: vk::PhysicalDevice,
    queue_family_index: u32,
) -> Result<DeviceGuard, VulkanDecodeError> {
    let queue_priorities = [1.0f32];
    let queue_create_infos = [vk::DeviceQueueCreateInfo::builder()
        .queue_family_index(queue_family_index)
        .queue_priorities(&queue_priorities)
        .build()];
    // Every codec extension is always enabled, regardless of which codec this
    // session actually uses — enabling an unused device extension is
    // harmless, and it avoids threading a codec parameter through device
    // creation just to pick between extension lists (matches
    // `mediaway-encoder-vulkan::session::create_logical_device`'s identical
    // choice).
    let extension_names: [*const std::ffi::c_char; 4] = [
        vk::KHR_VIDEO_QUEUE_EXTENSION.name.as_ptr(),
        vk::KHR_VIDEO_DECODE_QUEUE_EXTENSION.name.as_ptr(),
        vk::KHR_VIDEO_DECODE_H264_EXTENSION.name.as_ptr(),
        vk::KHR_VIDEO_DECODE_H265_EXTENSION.name.as_ptr(),
    ];
    let create_info = vk::DeviceCreateInfo::builder()
        .queue_create_infos(&queue_create_infos)
        .enabled_extension_names(&extension_names);
    // SAFETY: `physical_device` came from `find_h264_decode_device`/
    // `find_hevc_decode_device` on this same `instance`; `create_info` and
    // everything it borrows stay alive for this call; no allocation
    // callbacks supplied.
    let device = unsafe { instance.create_device(physical_device, &create_info, None) }.map_err(
        |result| VulkanDecodeError::VkCall {
            call: "vkCreateDevice",
            result,
        },
    )?;
    Ok(DeviceGuard { device })
}

pub(crate) fn find_memory_type(
    memory_properties: &vk::PhysicalDeviceMemoryProperties,
    type_bits: u32,
    required: vk::MemoryPropertyFlags,
) -> Result<u32, VulkanDecodeError> {
    for i in 0..memory_properties.memory_type_count {
        let bit_set = (type_bits >> i) & 1 == 1;
        let props_match = memory_properties.memory_types[i as usize]
            .property_flags
            .contains(required);
        if bit_set && props_match {
            return Ok(i);
        }
    }
    Err(VulkanDecodeError::NoMemoryType {
        type_bits,
        required,
    })
}

/// One video decode session (`VkVideoSessionKHR`) plus every
/// `vkGetVideoSessionMemoryRequirementsKHR`-requested `VkDeviceMemory` bound
/// to it. Mirrors `mediaway-encoder-vulkan::session_encode::create_video_session`
/// closely (decode-flavored: `max_dpb_slots`/`max_active_reference_pictures`
/// come from the parsed SPS, not fixed at `1`/`0`).
pub(crate) fn create_video_session(
    decode_device: &DecodeDevice<'_>,
    memory_properties: &vk::PhysicalDeviceMemoryProperties,
    profile: &mut DecodeProfile,
    capabilities: &Capabilities,
    coded_extent: vk::Extent2D,
    picture_format: vk::Format,
    max_dpb_slots: u32,
    max_active_reference_pictures: u32,
) -> Result<(vk::VideoSessionKHR, Vec<vk::DeviceMemory>), VulkanDecodeError> {
    let device = decode_device.device;
    let profile_info = profile.info();
    let create_info = vk::VideoSessionCreateInfoKHR::builder()
        .queue_family_index(decode_device.queue_family_index)
        .video_profile(&profile_info)
        .picture_format(picture_format)
        .max_coded_extent(coded_extent)
        .reference_picture_format(picture_format)
        .max_dpb_slots(max_dpb_slots)
        .max_active_reference_pictures(max_active_reference_pictures)
        .std_header_version(&capabilities.std_header_version);
    // SAFETY: `create_info` and everything it chains/borrows are alive for
    // this single synchronous call; no allocator callbacks supplied.
    let session =
        unsafe { device.create_video_session_khr(&create_info, None) }.map_err(|result| {
            VulkanDecodeError::VkCall {
                call: "vkCreateVideoSessionKHR",
                result,
            }
        })?;

    // SAFETY: `session` was just created on this `device`.
    let reqs =
        unsafe { device.get_video_session_memory_requirements_khr(session) }.map_err(|result| {
            VulkanDecodeError::VkCall {
                call: "vkGetVideoSessionMemoryRequirementsKHR",
                result,
            }
        })?;

    let mut memories = Vec::with_capacity(reqs.len());
    for req in &reqs {
        // A `VkVideoSessionKHR`'s per-bind-index memory requirements are
        // opaque driver-internal state — no specific property requirement
        // here (see `mediaway-encoder-vulkan::session_encode`'s identical
        // finding on this workspace's reference RTX 4090: requiring
        // `DEVICE_LOCAL` is wrong for at least one bind index).
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
            VulkanDecodeError::VkCall {
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
        VulkanDecodeError::VkCall {
            call: "vkBindVideoSessionMemoryKHR",
            result,
        }
    })?;

    Ok((session, memories))
}

/// Creates `VkVideoSessionParametersKHR` from one H.264 SPS + one PPS (this
/// crate never creates more than one of each — see `decoder.rs`).
pub(crate) fn create_session_parameters_h264(
    decode_device: &DecodeDevice<'_>,
    session: vk::VideoSessionKHR,
    sps: &vulkanalia::vk::video::StdVideoH264SequenceParameterSet,
    pps: &vulkanalia::vk::video::StdVideoH264PictureParameterSet,
) -> Result<vk::VideoSessionParametersKHR, VulkanDecodeError> {
    let device = decode_device.device;
    let add_info = vk::VideoDecodeH264SessionParametersAddInfoKHR::builder()
        .std_sp_ss(std::slice::from_ref(sps))
        .std_pp_ss(std::slice::from_ref(pps));
    let mut h264_create_info = vk::VideoDecodeH264SessionParametersCreateInfoKHR::builder()
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
        VulkanDecodeError::VkCall {
            call: "vkCreateVideoSessionParametersKHR",
            result,
        }
    })
}

/// HEVC sibling of [`create_session_parameters_h264`] — a third parameter
/// set (VPS) alongside SPS/PPS, otherwise identical shape (mirrors
/// `mediaway-encoder-vulkan::session_encode::create_session_parameters_hevc`).
pub(crate) fn create_session_parameters_hevc(
    decode_device: &DecodeDevice<'_>,
    session: vk::VideoSessionKHR,
    vps: &vulkanalia::vk::video::StdVideoH265VideoParameterSet,
    sps: &vulkanalia::vk::video::StdVideoH265SequenceParameterSet,
    pps: &vulkanalia::vk::video::StdVideoH265PictureParameterSet,
) -> Result<vk::VideoSessionParametersKHR, VulkanDecodeError> {
    let device = decode_device.device;
    let add_info = vk::VideoDecodeH265SessionParametersAddInfoKHR::builder()
        .std_vp_ss(std::slice::from_ref(vps))
        .std_sp_ss(std::slice::from_ref(sps))
        .std_pp_ss(std::slice::from_ref(pps));
    let mut hevc_create_info = vk::VideoDecodeH265SessionParametersCreateInfoKHR::builder()
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
        VulkanDecodeError::VkCall {
            call: "vkCreateVideoSessionParametersKHR",
            result,
        }
    })
}
