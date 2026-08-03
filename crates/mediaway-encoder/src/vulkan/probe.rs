//! Real `vulkanalia`-based Vulkan instance / physical-device / queue-family
//! probe for `VK_KHR_video_encode_queue` support.
//!
//! Hardware-verified 2026-07-29 against a real NVIDIA RTX 4090 + Intel UHD 770
//! — see the crate root docs' "Hardware-verified" section.

#![allow(unsafe_code)]

use std::ffi::CStr;

use thiserror::Error;
use vulkanalia::vk;
use vulkanalia::vk::{HasBuilder, InstanceV1_0};

/// One physical device's Vulkan Video encode queue-family findings.
#[derive(Debug, Clone)]
pub struct VulkanEncodeCapability {
    /// Driver-reported device name (`VkPhysicalDeviceProperties::deviceName`).
    pub device_name: String,
    /// `VkPhysicalDeviceType` (discrete GPU, integrated GPU, …).
    pub device_type: vk::PhysicalDeviceType,
    /// Index of the first queue family advertising
    /// `VK_VIDEO_CODEC_OPERATION_ENCODE_H264_BIT_KHR`, if any.
    pub h264_encode_queue_family: Option<u32>,
    /// Index of the first queue family advertising
    /// `VK_VIDEO_CODEC_OPERATION_ENCODE_H265_BIT_KHR`, if any.
    pub h265_encode_queue_family: Option<u32>,
}

/// Failures opening the Vulkan loader / instance for the probe.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum VulkanEncodeProbeError {
    /// No usable Vulkan loader (`vulkan-1.dll` / `libvulkan.so.1`) was found, or
    /// it could not be resolved via `libloading` at runtime.
    // `Box<dyn LoaderError>` cannot carry `#[source]` here: `LoaderError` only has
    // `std::error::Error` as a *supertrait*, and this workspace's pinned Rust
    // edition predates trait-upcasting stabilization, so thiserror's derive can't
    // obtain a `&dyn Error` from an already-erased `Box<dyn LoaderError>`. Display
    // still carries the full message via the `Error` supertrait's `Display` bound.
    #[error("failed to load the Vulkan loader: {0}")]
    Loader(Box<dyn vulkanalia::loader::LoaderError>),
    /// `vkCreateInstance` failed (out-of-memory, a driver-layer rejection,
    /// or — before this crate's 2026-07-29 hardware-verification fix — a
    /// device extension incorrectly requested at the instance level).
    #[error("vkCreateInstance failed: {0:?}")]
    CreateInstance(vk::ErrorCode),
    /// `vkEnumeratePhysicalDevices` failed.
    #[error("vkEnumeratePhysicalDevices failed: {0:?}")]
    EnumeratePhysicalDevices(vk::ErrorCode),
}

/// RAII guard so `vkDestroyInstance` runs on every return path (success, an
/// early `?` return, or panic-unwind) instead of relying on the caller to
/// remember cleanup — the same "typed session over a bare handle" shape the
/// rest of the workspace's platform backends use for OS resources.
struct InstanceGuard {
    instance: vulkanalia::Instance,
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        // SAFETY: `self.instance` was created by `probe_video_encode_queue_families`
        // and this guard is its sole owner (never cloned or returned to the
        // caller), so no other code can still be using it when this `Drop` runs.
        // No allocation callbacks were supplied at `create_instance`, so passing
        // `None` here matches that.
        unsafe { self.instance.destroy_instance(None) };
    }
}

/// Create a throwaway Vulkan instance, enumerate every physical device, and
/// report which queue families advertise H.264 / H.265 **encode** support.
///
/// This is a **capability probe only** — it does not create a logical device,
/// a video session, or encode anything. See the crate root docs and ADR-0001
/// for what is (and, this stage, is not) implemented on top of this.
///
/// # Errors
///
/// Returns [`VulkanEncodeProbeError`] when no Vulkan loader is present, or
/// `vkCreateInstance` / `vkEnumeratePhysicalDevices` fails. An empty
/// `Ok(Vec::new())` (zero physical devices) is not an error — some hosts
/// legitimately expose no Vulkan-capable device.
pub fn probe_video_encode_queue_families()
-> Result<Vec<VulkanEncodeCapability>, VulkanEncodeProbeError> {
    // SAFETY: `LibloadingLoader` dynamically resolves the system Vulkan loader
    // via `libloading` (this crate's `libloading` feature); the resulting
    // function pointers are only ever invoked through this `Entry` and the
    // `Instance` derived from it below.
    let loader = unsafe { vulkanalia::loader::LibloadingLoader::new(vulkanalia::loader::LIBRARY) }
        .map_err(|error| VulkanEncodeProbeError::Loader(error.into()))?;
    let entry =
        unsafe { vulkanalia::Entry::new(loader) }.map_err(VulkanEncodeProbeError::Loader)?;

    let app_info = vk::ApplicationInfo::builder()
        .application_name(b"mediaway-encoder-vulkan-probe\0")
        .api_version(vk::make_version(1, 3, 0));

    // `VK_KHR_video_queue` is a **device** extension (per the Vulkan registry —
    // it defines `VkVideoSessionKHR` and friends), not an instance one, so it
    // has no place in `InstanceCreateInfo::enabled_extension_names` — an
    // earlier version of this probe requested it there and every real driver
    // correctly rejected it with `VK_ERROR_EXTENSION_NOT_PRESENT` (caught by
    // running this against real hardware). Chaining `QueueFamilyVideoPropertiesKHR`
    // onto `get_physical_device_queue_family_properties2` below needs no
    // instance-level extension at all once the instance is created at API 1.3
    // (`get_physical_device_queue_family_properties2` is core since 1.1) — no
    // device extension needs enabling either, since this probe only *queries*
    // capabilities and never records a video command.
    let create_info = vk::InstanceCreateInfo::builder().application_info(&app_info);

    // SAFETY: `create_info` borrows `app_info`, alive for the duration of this
    // call; no allocation callbacks supplied.
    let instance = unsafe { entry.create_instance(&create_info, None) }
        .map_err(VulkanEncodeProbeError::CreateInstance)?;
    let guard = InstanceGuard { instance };

    // SAFETY: `guard.instance` was just created above by this same function and
    // is still valid (not yet dropped).
    let physical_devices = unsafe { guard.instance.enumerate_physical_devices() }
        .map_err(VulkanEncodeProbeError::EnumeratePhysicalDevices)?;

    let mut results = Vec::with_capacity(physical_devices.len());
    for physical_device in physical_devices {
        results.push(probe_one_device(&guard.instance, physical_device));
    }

    Ok(results)
}

/// Query one physical device's name/type and its queue families' video-encode
/// codec support.
fn probe_one_device(
    instance: &vulkanalia::Instance,
    physical_device: vk::PhysicalDevice,
) -> VulkanEncodeCapability {
    // SAFETY: `physical_device` came from `enumerate_physical_devices` on this
    // same `instance` immediately before this function is called.
    let props = unsafe { instance.get_physical_device_properties(physical_device) };
    let device_name = device_name_to_string(&props.device_name[..]);

    // SAFETY: `physical_device` is the same handle queried above, on the same
    // `instance`.
    let family_count =
        unsafe { instance.get_physical_device_queue_family_properties(physical_device) }.len();

    // Each entry chains a `QueueFamilyVideoPropertiesKHR` for the driver to fill
    // in. `InstanceV1_1::get_physical_device_queue_family_properties2` is a
    // convenience wrapper with no pNext-chain support (it always builds bare
    // `QueueFamilyProperties2::default()` internally) — the raw command table
    // entry is called directly instead so this crate's own pre-chained array
    // is what the driver writes into, mirroring the wrapper's own two-call
    // shape (`commands()` is part of `InstanceV1_0`'s public trait surface).
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

    let h264_encode_queue_family =
        find_family_with_codec(&video_props, vk::VideoCodecOperationFlagsKHR::ENCODE_H264);
    let h265_encode_queue_family =
        find_family_with_codec(&video_props, vk::VideoCodecOperationFlagsKHR::ENCODE_H265);

    VulkanEncodeCapability {
        device_name,
        device_type: props.device_type,
        h264_encode_queue_family,
        h265_encode_queue_family,
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
    // Queue family counts are always small (single digits in practice); fall
    // back to `u32::MAX` rather than an `as` truncation or `unwrap`/`expect`
    // (denied by workspace lints) in the unreachable overflow case.
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
