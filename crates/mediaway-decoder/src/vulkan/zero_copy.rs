//! Constructs [`GpuBufferHandle::Vulkan`] for
//! [`crate::VideoOutputPreference::ZeroCopyGpu`].
//!
//! **This crate's chosen encoding** (the field doc in `mediaway-common::gpu`
//! calls both fields backend-defined "opaque cookie"s — this is the first
//! real consumer, so this crate picks a concrete meaning): `image` is the raw
//! `VkImage` handle of `resources.dpb_image` (one `2D_ARRAY` image,
//! `dpb_slot_count` layers, shared by every DPB slot — see
//! `session_command.rs::create_dpb_image`'s doc for why a single shared
//! `2D_ARRAY` view, not one view per layer, is this crate's real working
//! shape). `memory` is repurposed to carry the array-layer index (not a
//! `VkDeviceMemory` — every layer shares one allocation, which alone would
//! not tell a caller which layer this frame's pixels are in): a real caller
//! needs both the image and the layer index to construct its own
//! single-layer view (or sample the shared array view at that layer).
//!
//! Backpressure contract (same "fail loudly, never silently overwrite" as
//! `dpb.rs`): whoever hands out the [`GpuBufferHandle`] this module builds
//! **must** call [`crate::vulkan::dpb::Dpb::mark_outstanding`] on that slot index
//! before returning it to the caller, and
//! [`crate::vulkan::dpb::Dpb::clear_outstanding`] once the caller has recycled the
//! frame (see `crate::VideoDecoder::poll_frame`'s documented
//! handle-lifetime contract) — `decoder.rs` is responsible for both calls;
//! this module only builds the handle value itself.

#![allow(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "workspace `unreachable_pub` policy (Cargo.toml) wants `pub(crate)` here; \
              clippy::pedantic's redundant_pub_crate disagrees for private modules — the \
              two lints are mutually exclusive for this shape, workspace policy wins"
)]

use mediaway_common::{GpuBufferHandle, NativeHandle};
use vulkanalia::vk;
use vulkanalia::vk::Handle;

use crate::vulkan::session::VulkanDecodeError;
use crate::vulkan::session_command::SessionResources;

/// Build the Zero-Copy handle for `resources.dpb_image`'s `slot_index` layer.
///
/// # Errors
///
/// Returns [`VulkanDecodeError::VkCall`] in the unreachable case that the
/// image's raw bits are `0` (a null `VkImage` would mean this crate's own
/// session setup is broken, never a caller input error).
pub(crate) fn build_handle(
    resources: &SessionResources,
    slot_index: u32,
) -> Result<GpuBufferHandle, VulkanDecodeError> {
    let image_bits = usize::try_from(resources.dpb_image.as_raw()).unwrap_or(0);
    let image = NativeHandle::new(image_bits).ok_or(VulkanDecodeError::VkCall {
        call: "zero_copy::build_handle (null VkImage)",
        result: vk::ErrorCode::UNKNOWN,
    })?;
    // `memory` carries the array-layer index, never `0`-valued for a real
    // slot (`NativeHandle` has no zero niche otherwise) — offset by one so
    // slot `0` still round-trips through `NativeHandle`'s non-zero
    // representation; callers must subtract one to recover the real layer.
    let memory = NativeHandle::new(slot_index as usize + 1).ok_or(VulkanDecodeError::VkCall {
        call: "zero_copy::build_handle (invalid slot index)",
        result: vk::ErrorCode::UNKNOWN,
    })?;
    Ok(GpuBufferHandle::Vulkan { image, memory })
}
