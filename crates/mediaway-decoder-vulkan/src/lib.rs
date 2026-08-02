//! Vulkan Video decode backend (`VK_KHR_video_decode_queue` via `vulkanalia`).
//!
//! **This round: H.264 (hardware-verified) + HEVC (sans-io real and tested,
//! GPU decode not yet hardware-verified).** AV1 (`adr/0001`'s wider design
//! scope) remains explicit follow-up work — no `av1_params.rs` exists yet.
//!
//! # Status (2026-07-30)
//!
//! [`probe::probe_video_decode_queue_families`] is real and hardware-verified:
//! this workspace's reference RTX 4090 **and** Intel UHD 770 both advertise
//! H.264/H.265/AV1 decode queue families (`cargo test -p mediaway-decoder-vulkan
//! probe -- --nocapture`).
//!
//! [`dpb`]/[`h264_params`]/[`h264_slice`]/[`hevc_params`]/[`hevc_slice`] (DPB
//! sliding-window bookkeeping, H.264 SPS/PPS/slice-header parsing +
//! `RefPicList0` construction, HEVC VPS/SPS/PPS/slice-segment-header parsing +
//! short-term RPS construction) are all real, sans-io, and unit-tested
//! independent of any GPU (62 tests).
//!
//! ## H.264 — hardware-verified end to end
//!
//! The Vulkan session/command-recording pipeline ([`VulkanVideoDecoder`])
//! compiles, passes `cargo clippy --all-targets` with zero warnings, and
//! **produces real, correct decoded NV12 output on real hardware**:
//! `tests/hardware_h264_decode.rs` decodes a hand-crafted IDR + P-frame
//! stream and asserts (hard `assert_eq!`, not a soft skip) exact pixel
//! values, including a real motion-compensated `P_Skip` reference read from
//! the DPB and genuinely new `I_PCM` content in the P-frame. Getting here
//! took three real bugs in this crate's own Vulkan Video command
//! construction (reference-slot `slotIndex` activation protocol, decode
//! target image layout, and a missing Annex-B start code in the uploaded
//! bitstream) — found by comparing field-by-field against `FFmpeg`'s own
//! working `vulkan_decode.c`/`vulkan_h264.c`.
//!
//! ## HEVC — sans-io real and tested, GPU decode not yet hardware-verified
//!
//! `hevc_params.rs`/`hevc_slice.rs` are real, tested, sans-io logic. The GPU
//! decode path (`decoder_hevc.rs`, `session_command_hevc.rs`) mirrors H.264's
//! now-verified command sequence and handles **IDR pictures only** this
//! round. `tests/hardware_hevc_decode.rs` chains this workspace's own
//! hardware-verified `mediaway-encoder-vulkan::VulkanVideoEncoder` into this
//! crate's decoder (no hand-written CABAC — HEVC has no CAVLC/`I_PCM` escape
//! like H.264's, so hand-constructing a bitstream is substantially harder).
//! Two real bugs were found and fixed this way (`HevcSps`/`HevcPps::to_std`
//! silently zeroing several `Std*Flags` bits regardless of what the real
//! encoder signaled), but the decoded picture still reads back all-zero —
//! **root cause not yet found**. The hardware test soft-skips loudly rather
//! than hard-failing the default suite.
//!
//! See [ADR-0001](../adr/0001-vulkan-video-decode.md)'s 2026-07-30 addenda for
//! the full diagnostic trail (both H.264's and HEVC's), and
//! [`docs/roadmap.md`](../docs/roadmap.md) for staged status.

#![allow(unsafe_code)]

mod cpu_readback;
mod decoder;
mod decoder_hevc;
pub mod dpb;
pub mod h264_params;
pub mod h264_slice;
pub mod hevc_params;
pub mod hevc_slice;
pub mod probe;
pub mod session;
mod session_command;
mod session_command_h264;
mod session_command_hevc;
mod zero_copy;

pub use decoder::VulkanVideoDecoder;

pub use probe::{
    VulkanDecodeCapability, VulkanDecodeProbeError, probe_video_decode_queue_families,
};
