//! Stage 1: a real, minimal, hardware-run `VK_KHR_video_encode_queue` H.264
//! encode of **one** synthetic all-intra frame.
//!
//! Covers session, session parameters, DPB, `vkCmdEncodeVideoKHR`, and
//! bitstream readback. See `adr/0001-vulkan-video-encode-ash-probe.md`'s
//! 2026-07-29 addendum for what this does and does not cover, and
//! [`h264_params`](crate::vulkan::h264_params) for the `StdVideoH264*` construction
//! it feeds.
//!
//! **Scope cuts (deliberate, see the ADR addendum for the honest reasoning):**
//! - No `VK_LAYER_KHRONOS_validation` — not installed on the machine this was
//!   written and run on (no Vulkan SDK; see the ADR). Every struct field
//!   below was instead checked against `ash`'s generated `Extends*` marker
//!   traits (ground truth from the Vulkan XML registry, not guesswork) and
//!   this machine's own `vulkaninfo --show-video-props` capability dump.
//! - No `VkQueryPoolVideoEncodeFeedbackCreateInfoKHR` byte-count query — the
//!   destination buffer is zero-filled before encoding and **not** trimmed to
//!   the driver's actual bytes-written count; callers scan for Annex-B start
//!   codes instead of trusting the buffer length. Good enough to prove real
//!   NAL bytes came back; not a byte-exact bitstream size.
//! - No per-object RAII beyond the Vulkan instance/device: this is a
//!   single-shot diagnostic path (see [`encode_synthetic_intra_frame`]), not
//!   a reusable encoder session. `SessionResources` (images, buffers, the
//!   video session itself) is torn down explicitly on the success path only;
//!   an early `?` on a rare failure leaks those handles until process exit
//!   the same way many one-shot Vulkan example programs do. Real DPB/session
//!   typestate (`VideoSession<S>`) is unstaged future work per the ADR sketch.

#![allow(unsafe_code)]

use thiserror::Error;
use vulkanalia::vk;
use vulkanalia::vk::{
    DeviceV1_0, HasBuilder, InstanceV1_0, KhrVideoQueueExtensionDeviceCommands,
    KhrVideoQueueExtensionInstanceCommands,
};

// This crate always asks the driver for its real minimum coded extent rather
// than picking one — on the RTX 4090 this workspace was written and
// hardware-verified against, that is `160x64` (`vulkaninfo
// --show-video-props`, H.264 Baseline 4:2:0 8-bit encode profile,
// 2026-07-29). `encode_synthetic_intra_frame` fails loudly via
// `VulkanEncodeSessionError::DegenerateCodedExtent` instead of guessing if a
// future driver ever reports a non-macroblock-aligned minimum.

/// A produced bitstream plus the coded picture size it was encoded at.
#[derive(Debug, Clone)]
pub struct EncodedFrame {
    /// Annex-B bytes: `vkGetEncodedVideoSessionParametersKHR`'s SPS+PPS NAL
    /// units, concatenated with the zero-filled `vkCmdEncodeVideoKHR`
    /// destination buffer contents (see the module doc's "Scope cuts").
    pub bitstream: Vec<u8>,
    /// Coded picture width in pixels (macroblock-aligned).
    pub coded_width: u32,
    /// Coded picture height in pixels (macroblock-aligned).
    pub coded_height: u32,
}

/// Failures from [`encode_synthetic_intra_frame`]. Every FFI call site maps
/// its `Result`/`VkResult` through this enum rather than `unwrap`/`expect`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum VulkanEncodeSessionError {
    /// No usable Vulkan loader was found, or it could not be resolved via
    /// `libloading` at runtime.
    // See `probe.rs`'s identical `Loader` variant for why this cannot carry
    // `#[source]`.
    #[error("failed to load the Vulkan loader: {0}")]
    Loader(Box<dyn vulkanalia::loader::LoaderError>),
    /// `vkCreateInstance` failed.
    #[error("vkCreateInstance failed: {0:?}")]
    CreateInstance(vk::ErrorCode),
    /// `vkEnumeratePhysicalDevices` failed.
    #[error("vkEnumeratePhysicalDevices failed: {0:?}")]
    EnumeratePhysicalDevices(vk::ErrorCode),
    /// No physical device on this host advertises an H.264 encode queue
    /// family (see `probe` for the same check as a standalone capability
    /// query).
    #[error(
        "no physical device advertises a VK_VIDEO_CODEC_OPERATION_ENCODE_H264_BIT_KHR queue family"
    )]
    NoEncodeCapableDevice,
    /// A raw Vulkan Video call returned a non-`SUCCESS` [`vk::Result`].
    #[error("{call} failed: {result:?}")]
    VkCall {
        /// The C function name that failed, e.g. `"vkCreateVideoSessionKHR"`.
        call: &'static str,
        /// The raw `VkResult` it returned.
        result: vk::ErrorCode,
    },
    /// `vkGetPhysicalDeviceVideoFormatPropertiesKHR` reported zero formats
    /// for the requested image usage under this crate's H.264 profile.
    #[error("driver reported no VK_KHR_video_encode_queue image format for usage {usage:?}")]
    NoVideoFormat {
        /// The image usage flags that had no matching format.
        usage: vk::ImageUsageFlags,
    },
    /// The driver's reported minimum coded extent is not a multiple of 16 —
    /// this crate's `McAlignedExtent` (and the whole SPS/PPS it builds)
    /// assumes a macroblock-aligned picture size.
    #[error("driver's reported min coded extent {width}x{height} is not macroblock-aligned")]
    DegenerateCodedExtent {
        /// Reported minimum coded width, in pixels.
        width: u32,
        /// Reported minimum coded height, in pixels.
        height: u32,
    },
    /// The caller-requested width/height falls outside
    /// `[min_coded_extent, max_coded_extent]` or is not a multiple of the
    /// driver's `picture_access_granularity` — a caller input error, distinct
    /// from [`Self::DegenerateCodedExtent`] (a driver-side finding).
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
}

/// RAII guard for the throwaway instance this module creates (mirrors
/// `probe::InstanceGuard`; not reused directly since a `vk::PhysicalDevice`
/// handle is only valid for the `VkInstance` that enumerated it, and
/// `probe`'s instance is destroyed before it returns).
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
        // SAFETY: sole owner; all resources built on `self.device` are
        // destroyed before this guard drops (see
        // `encode_synthetic_intra_frame`'s explicit teardown block on the
        // success path — see the module doc's "Scope cuts" for the error-path
        // caveat).
        unsafe { self.device.destroy_device(None) };
    }
}

/// Every handle [`encode_synthetic_intra_frame`] allocates on top of the
/// logical device. All fields default to the Vulkan null handle
/// (`vkDestroy*`/`vkFree*` on a null handle is a documented no-op), so
/// destroying every field unconditionally — even ones a given run never
/// populated — is well-defined.
#[derive(Default)]
pub(crate) struct SessionResources {
    pub(crate) command_pool: vk::CommandPool,
    pub(crate) session: vk::VideoSessionKHR,
    pub(crate) session_memories: Vec<vk::DeviceMemory>,
    pub(crate) session_parameters: vk::VideoSessionParametersKHR,
    pub(crate) input_image_view: vk::ImageView,
    pub(crate) input_image: vk::Image,
    pub(crate) input_image_memory: vk::DeviceMemory,
    pub(crate) dpb_image_view: vk::ImageView,
    pub(crate) dpb_image: vk::Image,
    pub(crate) dpb_image_memory: vk::DeviceMemory,
    pub(crate) staging_buffer: vk::Buffer,
    pub(crate) staging_memory: vk::DeviceMemory,
    pub(crate) dst_buffer: vk::Buffer,
    pub(crate) dst_memory: vk::DeviceMemory,
    pub(crate) fence: vk::Fence,
    /// `VK_QUERY_TYPE_VIDEO_ENCODE_FEEDBACK_KHR`, one query slot, requesting
    /// only `BITSTREAM_BYTES_WRITTEN` — the driver's real per-frame
    /// compressed byte count, read back after every encode so packets carry
    /// exactly that many bytes instead of the whole zero-padded destination
    /// buffer (see `session_command.rs::record_video_coding`).
    pub(crate) encode_feedback_query_pool: vk::QueryPool,
}

impl SessionResources {
    /// Explicit best-effort teardown — see the module doc's "Scope cuts" for
    /// why this isn't a `Drop` impl.
    pub(crate) fn destroy(&self, device: &vulkanalia::Device) {
        // SAFETY: every handle here was either created by this same `device`
        // earlier in `encode_synthetic_intra_frame`, or is the
        // default-initialized null handle (a documented no-op for every
        // `vkDestroy*`/`vkFree*` call below). Called once, after the command
        // buffer's fence has already been waited on, so nothing here is still
        // in use by the GPU.
        unsafe {
            device.destroy_query_pool(self.encode_feedback_query_pool, None);
            device.destroy_fence(self.fence, None);
            device.destroy_buffer(self.dst_buffer, None);
            device.free_memory(self.dst_memory, None);
            device.destroy_buffer(self.staging_buffer, None);
            device.free_memory(self.staging_memory, None);
            device.destroy_image_view(self.dpb_image_view, None);
            device.destroy_image(self.dpb_image, None);
            device.free_memory(self.dpb_image_memory, None);
            device.destroy_image_view(self.input_image_view, None);
            device.destroy_image(self.input_image, None);
            device.free_memory(self.input_image_memory, None);
            device.destroy_video_session_parameters_khr(self.session_parameters, None);
            for &memory in &self.session_memories {
                device.free_memory(memory, None);
            }
            device.destroy_video_session_khr(self.session, None);
            device.destroy_command_pool(self.command_pool, None);
        }
    }
}

/// Creates a throwaway instance the same way `probe.rs` does (see that
/// module's comment on why `VK_KHR_video_queue` must not be requested as an
/// *instance* extension).
pub(crate) fn create_instance()
-> Result<(vulkanalia::Entry, InstanceGuard), VulkanEncodeSessionError> {
    // SAFETY: `LibloadingLoader` dynamically resolves the system Vulkan loader;
    // the resulting function pointers are only invoked through this `Entry`
    // and instances derived from it.
    let loader = unsafe { vulkanalia::loader::LibloadingLoader::new(vulkanalia::loader::LIBRARY) }
        .map_err(|error| VulkanEncodeSessionError::Loader(error.into()))?;
    let entry =
        unsafe { vulkanalia::Entry::new(loader) }.map_err(VulkanEncodeSessionError::Loader)?;
    let app_info = vk::ApplicationInfo::builder()
        .application_name(b"mediaway-encoder-vulkan-session\0")
        .api_version(vk::make_version(1, 3, 0));
    let create_info = vk::InstanceCreateInfo::builder().application_info(&app_info);
    // SAFETY: `create_info` borrows `app_info`, alive for this call; no
    // allocation callbacks supplied.
    let instance = unsafe { entry.create_instance(&create_info, None) }
        .map_err(VulkanEncodeSessionError::CreateInstance)?;
    Ok((entry, InstanceGuard { instance }))
}

/// Finds the first physical device + queue family advertising `op` (mirrors
/// `probe::probe_one_device`'s query, duplicated rather than shared — see
/// the module doc: this module intentionally does not depend on `probe`'s
/// short-lived instance). [`find_h264_encode_device`]/[`find_hevc_encode_device`]
/// are thin codec-specific wrappers.
fn find_encode_device(
    instance: &vulkanalia::Instance,
    op: vk::VideoCodecOperationFlagsKHR,
) -> Result<(vk::PhysicalDevice, u32), VulkanEncodeSessionError> {
    // SAFETY: `instance` is a live instance owned by the caller for the
    // duration of this call.
    let physical_devices = unsafe { instance.enumerate_physical_devices() }
        .map_err(VulkanEncodeSessionError::EnumeratePhysicalDevices)?;

    for physical_device in physical_devices {
        // SAFETY: `physical_device` was just enumerated from `instance`.
        let family_count =
            unsafe { instance.get_physical_device_queue_family_properties(physical_device) }.len();
        // `InstanceV1_1::get_physical_device_queue_family_properties2` has no
        // pNext-chain support (see `probe.rs`'s identical query) — the raw
        // command table entry is called directly instead.
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
    Err(VulkanEncodeSessionError::NoEncodeCapableDevice)
}

pub(crate) fn find_h264_encode_device(
    instance: &vulkanalia::Instance,
) -> Result<(vk::PhysicalDevice, u32), VulkanEncodeSessionError> {
    find_encode_device(instance, vk::VideoCodecOperationFlagsKHR::ENCODE_H264)
}

pub(crate) fn find_hevc_encode_device(
    instance: &vulkanalia::Instance,
) -> Result<(vk::PhysicalDevice, u32), VulkanEncodeSessionError> {
    find_encode_device(instance, vk::VideoCodecOperationFlagsKHR::ENCODE_H265)
}

pub(crate) fn find_av1_encode_device(
    instance: &vulkanalia::Instance,
) -> Result<(vk::PhysicalDevice, u32), VulkanEncodeSessionError> {
    find_encode_device(instance, vk::VideoCodecOperationFlagsKHR::ENCODE_AV1)
}

/// This crate's fixed per-codec encode profile — H.264 Baseline, HEVC Main,
/// or AV1 Main, all 4:2:0 8-bit (matches NV12). An enum (not a shared struct)
/// since each codec's `*ProfileInfoKHR` pNext payload is a distinct C-union
/// arm, same reasoning as `mediaway-encoder-windows`'s D3D12 backend's
/// `GopStructure` enum.
pub(crate) enum EncodeProfile {
    H264(vk::VideoEncodeH264ProfileInfoKHR),
    Hevc(vk::VideoEncodeH265ProfileInfoKHR),
    Av1(vk::VideoEncodeAV1ProfileInfoKHR),
}

impl EncodeProfile {
    pub(crate) fn new_h264() -> Self {
        Self::H264(
            vk::VideoEncodeH264ProfileInfoKHR::builder()
                .std_profile_idc(h264_params_profile_idc())
                .build(),
        )
    }

    pub(crate) fn new_hevc() -> Self {
        Self::Hevc(
            vk::VideoEncodeH265ProfileInfoKHR::builder()
                .std_profile_idc(hevc_params_profile_idc())
                .build(),
        )
    }

    pub(crate) fn new_av1() -> Self {
        Self::Av1(
            vk::VideoEncodeAV1ProfileInfoKHR::builder()
                .std_profile(vulkanalia::vk::video::STD_VIDEO_AV1_PROFILE_MAIN)
                .build(),
        )
    }

    pub(crate) fn info(&mut self) -> vk::VideoProfileInfoKHR {
        let base = vk::VideoProfileInfoKHR::builder()
            .chroma_subsampling(vk::VideoChromaSubsamplingFlagsKHR::_420)
            .luma_bit_depth(vk::VideoComponentBitDepthFlagsKHR::_8)
            .chroma_bit_depth(vk::VideoComponentBitDepthFlagsKHR::_8);
        match self {
            Self::H264(h264) => base
                .video_codec_operation(vk::VideoCodecOperationFlagsKHR::ENCODE_H264)
                .push_next(h264)
                .build(),
            Self::Hevc(hevc) => base
                .video_codec_operation(vk::VideoCodecOperationFlagsKHR::ENCODE_H265)
                .push_next(hevc)
                .build(),
            Self::Av1(av1) => base
                .video_codec_operation(vk::VideoCodecOperationFlagsKHR::ENCODE_AV1)
                .push_next(av1)
                .build(),
        }
    }
}

const fn h264_params_profile_idc() -> vulkanalia::vk::video::StdVideoH264ProfileIdc {
    vulkanalia::vk::video::STD_VIDEO_H264_PROFILE_IDC_BASELINE
}

const fn hevc_params_profile_idc() -> vulkanalia::vk::video::StdVideoH265ProfileIdc {
    vulkanalia::vk::video::STD_VIDEO_H265_PROFILE_IDC_MAIN
}

/// Real capability numbers this crate needs from
/// `vkGetPhysicalDeviceVideoCapabilitiesKHR` — coded-extent bounds and the
/// exact driver-reported `stdHeaderVersion` `VkVideoSessionCreateInfoKHR`
/// must echo back unchanged.
pub(crate) struct Capabilities {
    pub(crate) min_coded_extent: vk::Extent2D,
    pub(crate) max_coded_extent: vk::Extent2D,
    /// Required alignment for both dimensions of any coded extent this
    /// profile accepts. Driver-reported, not a fixed constant — this
    /// crate's reference RTX 4090 reports `16x16` for H.264 (macroblocks)
    /// but `32x32` for HEVC, not `16x16`/HEVC's own minimum CTU size —
    /// callers must query per codec, never assume the two match.
    pub(crate) picture_access_granularity: vk::Extent2D,
    /// Required alignment for the destination bitstream buffer's byte size.
    pub(crate) min_bitstream_buffer_size_alignment: vk::DeviceSize,
    pub(crate) std_header_version: vk::ExtensionProperties,
}

impl Capabilities {
    /// Validates a caller-requested coded extent against this driver's
    /// reported bounds and alignment — the check
    /// [`encode_synthetic_intra_frame`](crate::vulkan::session_encode::encode_synthetic_intra_frame)
    /// never needed (it always used [`Self::min_coded_extent`] directly, which
    /// is trivially in-range) but a real multi-resolution
    /// [`crate::vulkan::encoder::VulkanVideoEncoder`] does.
    pub(crate) const fn validate_requested_extent(
        &self,
        width: u32,
        height: u32,
    ) -> Result<(), VulkanEncodeSessionError> {
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
        Err(VulkanEncodeSessionError::UnsupportedResolution {
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

pub(crate) fn query_capabilities(
    instance: &vulkanalia::Instance,
    physical_device: vk::PhysicalDevice,
    profile: &mut EncodeProfile,
) -> Result<Capabilities, VulkanEncodeSessionError> {
    // The codec-specific capabilities struct chained below **must** match
    // `profile`'s own codec — chaining H.264's capabilities struct while
    // querying an HEVC profile made a real driver reject the whole call
    // outright (`vkGetPhysicalDeviceVideoCapabilitiesKHR` returning an error
    // `VkResult`), found on this crate's reference RTX 4090 while adding
    // HEVC support. Determined before `profile.info()` mutably borrows
    // `profile` for the rest of this function.
    let is_hevc = matches!(profile, EncodeProfile::Hevc(_));
    let is_av1 = matches!(profile, EncodeProfile::Av1(_));
    // `profile_info`'s `p_next` points at `profile`'s own enum-variant field
    // (see `EncodeProfile::info`'s doc) — `profile` outlives this whole
    // function, so the pointer stays valid for every use below.
    let profile_info = profile.info();
    let mut h264_caps = vk::VideoEncodeH264CapabilitiesKHR::default();
    let mut hevc_caps = vk::VideoEncodeH265CapabilitiesKHR::default();
    let mut av1_caps = vk::VideoEncodeAV1CapabilitiesKHR::default();
    let mut encode_caps = vk::VideoEncodeCapabilitiesKHR::default();
    // `encode_caps` and the codec-specific struct both chain directly onto
    // `caps` (a flat pNext list, not nested) — confirmed via the generated
    // `ExtendsVideoCapabilitiesKHR` marker trait, implemented for
    // `VideoEncodeCapabilitiesKHR`/`VideoEncodeH264CapabilitiesKHR`/
    // `VideoEncodeH265CapabilitiesKHR`/`VideoEncodeAV1CapabilitiesKHR` all
    // directly against `VideoCapabilitiesKHR`, not against each other.
    let mut caps_builder = vk::VideoCapabilitiesKHR::builder().push_next(&mut encode_caps);
    caps_builder = if is_av1 {
        caps_builder.push_next(&mut av1_caps)
    } else if is_hevc {
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
    result.map_err(|result| VulkanEncodeSessionError::VkCall {
        call: "vkGetPhysicalDeviceVideoCapabilitiesKHR",
        result,
    })?;
    if caps.min_coded_extent.width % 16 != 0 || caps.min_coded_extent.height % 16 != 0 {
        return Err(VulkanEncodeSessionError::DegenerateCodedExtent {
            width: caps.min_coded_extent.width,
            height: caps.min_coded_extent.height,
        });
    }
    Ok(Capabilities {
        min_coded_extent: caps.min_coded_extent,
        max_coded_extent: caps.max_coded_extent,
        picture_access_granularity: caps.picture_access_granularity,
        min_bitstream_buffer_size_alignment: caps.min_bitstream_buffer_size_alignment,
        std_header_version: caps.std_header_version,
    })
}

/// Queries the concrete [`vk::Format`] the driver wants for a given video
/// image usage (encode source or DPB) under this profile — 2-call pattern
/// (`vkGetPhysicalDeviceVideoFormatPropertiesKHR`), first entry only.
pub(crate) fn query_video_format(
    instance: &vulkanalia::Instance,
    physical_device: vk::PhysicalDevice,
    profile: &mut EncodeProfile,
    usage: vk::ImageUsageFlags,
) -> Result<vk::Format, VulkanEncodeSessionError> {
    // `profile_info`'s `p_next` points at `profile`'s own field — `profile`
    // outlives this whole function (see `query_capabilities`'s identical note).
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
    .map_err(|result| VulkanEncodeSessionError::VkCall {
        call: "vkGetPhysicalDeviceVideoFormatPropertiesKHR",
        result,
    })?;
    formats
        .first()
        .map(|f| f.format)
        .ok_or(VulkanEncodeSessionError::NoVideoFormat { usage })
}

pub(crate) fn create_logical_device(
    instance: &vulkanalia::Instance,
    physical_device: vk::PhysicalDevice,
    queue_family_index: u32,
) -> Result<DeviceGuard, VulkanEncodeSessionError> {
    let queue_priorities = [1.0f32];
    let queue_create_infos = [vk::DeviceQueueCreateInfo::builder()
        .queue_family_index(queue_family_index)
        .queue_priorities(&queue_priorities)
        .build()];
    // Every codec extension is always enabled, regardless of which codec
    // this session actually uses — enabling an unused device extension is
    // harmless, and it avoids threading a codec parameter through device
    // creation just to pick between three single-string extension lists.
    let extension_names: [*const std::ffi::c_char; 5] = [
        vk::KHR_VIDEO_QUEUE_EXTENSION.name.as_ptr(),
        vk::KHR_VIDEO_ENCODE_QUEUE_EXTENSION.name.as_ptr(),
        vk::KHR_VIDEO_ENCODE_H264_EXTENSION.name.as_ptr(),
        vk::KHR_VIDEO_ENCODE_H265_EXTENSION.name.as_ptr(),
        vk::KHR_VIDEO_ENCODE_AV1_EXTENSION.name.as_ptr(),
    ];
    let create_info = vk::DeviceCreateInfo::builder()
        .queue_create_infos(&queue_create_infos)
        .enabled_extension_names(&extension_names);
    // SAFETY: `physical_device` came from `find_h264_encode_device`/
    // `find_hevc_encode_device` on this same `instance`; `create_info` and
    // everything it borrows stay alive for this call; no allocation
    // callbacks supplied.
    let device = unsafe { instance.create_device(physical_device, &create_info, None) }.map_err(
        |result| VulkanEncodeSessionError::VkCall {
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
) -> Result<u32, VulkanEncodeSessionError> {
    for i in 0..memory_properties.memory_type_count {
        let bit_set = (type_bits >> i) & 1 == 1;
        let props_match = memory_properties.memory_types[i as usize]
            .property_flags
            .contains(required);
        if bit_set && props_match {
            return Ok(i);
        }
    }
    Err(VulkanEncodeSessionError::NoMemoryType {
        type_bits,
        required,
    })
}

/// Bundles the device-derived handles `session_encode`/`session_command`
/// helpers need, so none of them individually trips
/// `clippy::too_many_arguments`.
pub(crate) struct EncodeDevice<'a> {
    pub(crate) device: &'a vulkanalia::Device,
    pub(crate) queue: vk::Queue,
    pub(crate) queue_family_index: u32,
}
