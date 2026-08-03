//! Real `vulkanalia`-based Vulkan instance / physical-device / queue-family
//! probe for `VK_KHR_video_decode_queue` support.
//!
//! Structurally identical to `mediaway-encoder-vulkan::probe::probe_video_encode_queue_families`
//! (Stage 0 of that crate's own ADR) — same instance/physical-device/
//! `vkGetPhysicalDeviceQueueFamilyProperties2` + `VkQueueFamilyVideoPropertiesKHR`
//! chain, decode operation-flag bits instead of encode ones. The already-known
//! bug that probe's own history recorded (`VK_KHR_video_queue` is a **device**
//! extension, not an instance one) is avoided here from the start — this
//! probe never requests any `VK_KHR_video_*` extension at the instance level.
//!
//! See crate root docs / `adr/0001`'s 2026-07-29 addendum for the real result
//! on this workspace's reference machine.

#![allow(unsafe_code)]

use std::ffi::CStr;

use thiserror::Error;
use vulkanalia::vk;
use vulkanalia::vk::{HasBuilder, InstanceV1_0};

/// One physical device's Vulkan Video **decode** queue-family findings.
#[derive(Debug, Clone)]
pub struct VulkanDecodeCapability {
    /// Driver-reported device name (`VkPhysicalDeviceProperties::deviceName`).
    pub device_name: String,
    /// `VkPhysicalDeviceType` (discrete GPU, integrated GPU, …).
    pub device_type: vk::PhysicalDeviceType,
    /// Index of the first queue family advertising
    /// `VK_VIDEO_CODEC_OPERATION_DECODE_H264_BIT_KHR`, if any.
    pub h264_decode_queue_family: Option<u32>,
    /// Index of the first queue family advertising
    /// `VK_VIDEO_CODEC_OPERATION_DECODE_H265_BIT_KHR`, if any (probed for
    /// completeness alongside H.264 — this crate implements H.264 only this
    /// round, see crate root docs).
    pub h265_decode_queue_family: Option<u32>,
    /// Index of the first queue family advertising
    /// `VK_VIDEO_CODEC_OPERATION_DECODE_AV1_BIT_KHR`, if any (probed for
    /// completeness; not implemented this round).
    pub av1_decode_queue_family: Option<u32>,
}

/// Failures opening the Vulkan loader / instance for the probe.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum VulkanDecodeProbeError {
    /// No usable Vulkan loader (`vulkan-1.dll` / `libvulkan.so.1`) was found,
    /// or it could not be resolved via `libloading` at runtime.
    // See `mediaway-encoder-vulkan::probe`'s identical variant for why this
    // cannot carry `#[source]` (no trait-upcasting on this workspace's pinned
    // edition).
    #[error("failed to load the Vulkan loader: {0}")]
    Loader(Box<dyn vulkanalia::loader::LoaderError>),
    /// `vkCreateInstance` failed.
    #[error("vkCreateInstance failed: {0:?}")]
    CreateInstance(vk::ErrorCode),
    /// `vkEnumeratePhysicalDevices` failed.
    #[error("vkEnumeratePhysicalDevices failed: {0:?}")]
    EnumeratePhysicalDevices(vk::ErrorCode),
}

/// RAII guard so `vkDestroyInstance` runs on every return path — mirrors
/// `mediaway-encoder-vulkan::probe::InstanceGuard` exactly (not shared: that
/// guard is private to its own crate).
struct InstanceGuard {
    instance: vulkanalia::Instance,
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        // SAFETY: `self.instance` was created by `probe_video_decode_queue_families`
        // and this guard is its sole owner (never cloned or returned to the
        // caller), so no other code can still be using it when this `Drop` runs.
        // No allocation callbacks were supplied at `create_instance`, so passing
        // `None` here matches that.
        unsafe { self.instance.destroy_instance(None) };
    }
}

/// Create a throwaway Vulkan instance, enumerate every physical device, and
/// report which queue families advertise H.264 / H.265 / AV1 **decode**
/// support.
///
/// This is a **capability probe only** — it does not create a logical
/// device, a video session, or decode anything.
///
/// # Errors
///
/// Returns [`VulkanDecodeProbeError`] when no Vulkan loader is present, or
/// `vkCreateInstance` / `vkEnumeratePhysicalDevices` fails. An empty
/// `Ok(Vec::new())` (zero physical devices) is not an error.
pub fn probe_video_decode_queue_families()
-> Result<Vec<VulkanDecodeCapability>, VulkanDecodeProbeError> {
    // SAFETY: `LibloadingLoader` dynamically resolves the system Vulkan loader
    // via `libloading` (this crate's `libloading` feature); the resulting
    // function pointers are only ever invoked through this `Entry` and the
    // `Instance` derived from it below.
    let loader = unsafe { vulkanalia::loader::LibloadingLoader::new(vulkanalia::loader::LIBRARY) }
        .map_err(|error| VulkanDecodeProbeError::Loader(error.into()))?;
    let entry =
        unsafe { vulkanalia::Entry::new(loader) }.map_err(VulkanDecodeProbeError::Loader)?;

    let app_info = vk::ApplicationInfo::builder()
        .application_name(b"mediaway-decoder-vulkan-probe\0")
        .api_version(vk::make_version(1, 3, 0));

    // `VK_KHR_video_queue`/`VK_KHR_video_decode_queue` are **device**
    // extensions, not instance ones (per the Vulkan registry) — not requested
    // here. `get_physical_device_queue_family_properties2` is core since 1.1,
    // so no instance extension is needed to query capabilities either.
    let create_info = vk::InstanceCreateInfo::builder().application_info(&app_info);

    // SAFETY: `create_info` borrows `app_info`, alive for the duration of this
    // call; no allocation callbacks supplied.
    let instance = unsafe { entry.create_instance(&create_info, None) }
        .map_err(VulkanDecodeProbeError::CreateInstance)?;
    let guard = InstanceGuard { instance };

    // SAFETY: `guard.instance` was just created above by this same function and
    // is still valid (not yet dropped).
    let physical_devices = unsafe { guard.instance.enumerate_physical_devices() }
        .map_err(VulkanDecodeProbeError::EnumeratePhysicalDevices)?;

    let mut results = Vec::with_capacity(physical_devices.len());
    for physical_device in physical_devices {
        results.push(probe_one_device(&guard.instance, physical_device));
    }

    Ok(results)
}

/// Query one physical device's name/type and its queue families' video-decode
/// codec support.
fn probe_one_device(
    instance: &vulkanalia::Instance,
    physical_device: vk::PhysicalDevice,
) -> VulkanDecodeCapability {
    // SAFETY: `physical_device` came from `enumerate_physical_devices` on this
    // same `instance` immediately before this function is called.
    let props = unsafe { instance.get_physical_device_properties(physical_device) };
    let device_name = device_name_to_string(&props.device_name[..]);

    // SAFETY: `physical_device` is the same handle queried above, on the same
    // `instance`.
    let family_count =
        unsafe { instance.get_physical_device_queue_family_properties(physical_device) }.len();

    // Same two-array chained-query shape as
    // `mediaway-encoder-vulkan::probe::probe_one_device` — see that
    // function's comment for why the raw command-table entry is called
    // directly instead of the no-pNext-support convenience wrapper.
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

    // SAFETY: `families2` has exactly `family_count` entries (queried just
    // above via the non-"2" query on the same `physical_device`), each chaining
    // a live `QueueFamilyVideoPropertiesKHR` from `video_props` — which outlives
    // this call — for the driver to write into. `family_count` was obtained from
    // this same physical device immediately above, so the array length matches
    // what the driver will report back.
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

    let h264_decode_queue_family =
        find_family_with_codec(&video_props, vk::VideoCodecOperationFlagsKHR::DECODE_H264);
    let h265_decode_queue_family =
        find_family_with_codec(&video_props, vk::VideoCodecOperationFlagsKHR::DECODE_H265);
    let av1_decode_queue_family =
        find_family_with_codec(&video_props, vk::VideoCodecOperationFlagsKHR::DECODE_AV1);

    VulkanDecodeCapability {
        device_name,
        device_type: props.device_type,
        h264_decode_queue_family,
        h265_decode_queue_family,
        av1_decode_queue_family,
    }
}

/// First queue-family index whose `video_codec_operations` includes `codec`.
fn find_family_with_codec(
    video_props: &[vk::QueueFamilyVideoPropertiesKHR],
    codec: vk::VideoCodecOperationFlagsKHR,
) -> Option<u32> {
    let index = video_props
        .iter()
        .position(|entry| entry.video_codec_operations.contains(codec))?;
    Some(u32::try_from(index).unwrap_or(u32::MAX))
}

/// Convert a driver-filled fixed `deviceName` buffer to a `String`.
fn device_name_to_string(name: &[std::ffi::c_char]) -> String {
    // SAFETY: the Vulkan spec guarantees `VkPhysicalDeviceProperties::deviceName`
    // is a NUL-terminated string within its fixed
    // `VK_MAX_PHYSICAL_DEVICE_NAME_SIZE` buffer.
    let cstr = unsafe { CStr::from_ptr(name.as_ptr()) };
    cstr.to_string_lossy().into_owned()
}

#[cfg(test)]
#[path = "probe_tests.rs"]
mod tests;
