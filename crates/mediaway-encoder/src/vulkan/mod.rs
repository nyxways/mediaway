//! Vulkan Video encode backend (`VK_KHR_video_encode_queue` via `ash`).
//!
//! **Stage 0 (2026-07-29):** a real `ash`-based **capability probe** — create
//! a Vulkan instance, enumerate physical devices, and report which queue
//! families advertise [`vulkanalia::vk::VideoCodecOperationFlagsKHR::ENCODE_H264`] /
//! `ENCODE_H265` (`vkGetPhysicalDeviceQueueFamilyProperties2` chained with
//! `VkQueueFamilyVideoPropertiesKHR`). See [`probe::probe_video_encode_queue_families`].
//!
//! **Stage 1 (same-day follow-up, 2026-07-29):** [`encode_synthetic_intra_frame`],
//! a real, minimal, hardware-run H.264 encode of one synthetic all-intra
//! frame combining a `VkVideoSessionKHR`, a `VkVideoSessionParametersKHR`
//! (real SPS/PPS), DPB/input images, one `vkCmdEncodeVideoKHR`, and bitstream
//! readback, verified on the same RTX 4090 the Stage 0 probe found. See
//! [`session`]/[`session_encode`]/[`session_command`] and
//! [`adr/0001-vulkan-video-encode-ash-probe.md`](../adr/0001-vulkan-video-encode-ash-probe.md)'s
//! 2026-07-29 addendum for exactly what this does and does not cover (it is
//! **not** a `crate::VideoEncoder` impl, not multi-frame, not
//! rate-controlled, and not Zero-Copy — see [`docs/roadmap.md`](../docs/roadmap.md)
//! for the staged plan and what remains).
//!
//! # Hardware-verified (2026-07-29)
//!
//! Written against `ash` 0.38's documented API, then actually compiled and run
//! against real hardware on a Windows machine with an NVIDIA RTX 4090 + Intel
//! UHD 770: `cargo test -p mediaway-encoder-vulkan -- --nocapture` reports
//! `NVIDIA GeForce RTX 4090 (DISCRETE_GPU) h264_encode_queue_family=Some(4)
//! h265_encode_queue_family=Some(4)` and `Intel(R) UHD Graphics 770
//! (INTEGRATED_GPU) h264_encode_queue_family=None h265_encode_queue_family=None`
//! (the Intel Windows Vulkan driver on this machine advertises no video-encode
//! queue — a real, machine-specific finding, not a probe bug). One real bug was
//! caught by this run and fixed: the first draft enabled `VK_KHR_video_queue`
//! (a **device** extension) in `InstanceCreateInfo`, which every driver
//! correctly rejected with `VK_ERROR_EXTENSION_NOT_PRESENT` — see the fix in
//! [`probe`]'s source comments.
//!
//! Stage 1's real result on this same machine — pass, fail, or blocked, with
//! exact `VkResult`s — is recorded in `adr/0001-vulkan-video-encode-ash-probe.md`'s
//! 2026-07-29 addendum, not summarized here (see that file for the honest
//! account this crate's own rules require).

#![allow(unsafe_code)]

mod av1_params;
mod encoder;
#[cfg(test)]
#[path = "encoder_tests.rs"]
mod encoder_tests;
mod h264_params;
mod hevc_params;
#[cfg(test)]
mod nal;
pub mod probe;
pub mod session;
mod session_command;
mod session_command_av1;
mod session_command_hevc;
mod session_encode;
#[cfg(test)]
#[path = "session_tests.rs"]
mod session_tests;

pub use encoder::VulkanVideoEncoder;
pub use probe::{
    VulkanEncodeCapability, VulkanEncodeProbeError, probe_video_encode_queue_families,
};
pub use session::{EncodedFrame, VulkanEncodeSessionError};
pub use session_encode::encode_synthetic_intra_frame;
