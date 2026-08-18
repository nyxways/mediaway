//! Vulkan Video decode backend (`VK_KHR_video_decode_queue` via `vulkanalia`).
//!
//! **This round: H.264 (general-GOP, hardware-verified) + HEVC (IDR-only,
//! hardware-verified) + AV1 (`KEY_FRAME`-only, single-tile, hardware-verified
//! — [`av1_params`], `adr/vulkan/0002`).**
//!
//! # Status (2026-08-19)
//!
//! [`probe::probe_video_decode_queue_families`] is real and hardware-verified:
//! this workspace's reference RTX 4090 **and** Intel UHD 770 both advertise
//! H.264/H.265/AV1 decode queue families.
//!
//! [`dpb`]/[`h264_params`]/[`h264_slice`]/[`hevc_params`]/[`hevc_slice`]/
//! [`av1_params`]/[`av1_refs`] (DPB sliding-window bookkeeping, per-codec
//! bitstream parsing, HEVC short-term RPS construction, AV1 OBU/sequence-
//! header/`KEY_FRAME`-uncompressed-header parsing) are all real, sans-io,
//! and unit-tested independent of any GPU.
//!
//! ## H.264 — hardware-verified end to end (general-GOP)
//!
//! `tests/vulkan/hardware_h264_decode.rs` decodes a hand-crafted IDR + P-frame
//! stream and asserts (hard `assert_eq!`, not a soft skip) exact pixel
//! values, including a real motion-compensated `P_Skip` reference read from
//! the DPB. Three real bugs in this crate's own Vulkan Video command
//! construction (reference-slot `slotIndex` activation protocol, decode
//! target image layout, a missing Annex-B start code) were found comparing
//! field-by-field against `FFmpeg`'s working `vulkan_decode.c`/`vulkan_h264.c`.
//!
//! ## HEVC — hardware-verified (IDR-only)
//!
//! `decoder_hevc.rs`/`session_command_hevc.rs` handle **IDR pictures only**
//! this round (P/B-slice HEVC decode is an explicit follow-up).
//! `tests/vulkan/hardware_hevc_decode.rs` chains this workspace's own
//! hardware-verified Vulkan HEVC **encoder** into this crate's decoder (no
//! hand-written CABAC). Root cause of an earlier all-zero-output bug: a
//! missing slice-header bit desyncing the driver's CABAC parser — see
//! [ADR-0001](../adr/0001-vulkan-video-decode.md)'s 2026-08-05 addendum.
//!
//! ## AV1 — hardware-verified (`KEY_FRAME`-only, single-tile)
//!
//! [`av1_params`]/`av1_params::av1_frame_header` parse the sequence header
//! and a `KEY_FRAME`'s full `uncompressed_header()` (segmentation,
//! quantization, loop filter, CDEF, loop restoration, tile info — all real,
//! not stubs). `decoder_av1.rs`/`session_command_av1.rs` handle `frame_type
//! == KEY_FRAME`/`show_frame == 1`/single-tile pictures only, rejecting
//! anything else with `DecodeError::Unsupported` — see
//! [ADR-0002](../adr/vulkan/0002-av1-decode-keyframe-first.md). No Annex-B
//! start code (AV1 has none); `frameHeaderOffset`/tile-offset framing is
//! this crate's own design (see `av1_frame_header.rs`'s module doc).
//! `tests/vulkan/hardware_av1_decode.rs` pushes a real `mediaway_sw::av1::Av1Encoder`
//! (`rav1e`-backed, pure-CPU) `KEY_FRAME` through the decoder and asserts
//! real decoded NV12 content — passed hardware verification on the first
//! attempt, unlike this workspace's AV1 Vulkan **encode** path (confirmed
//! driver-blocked, a different extension family — this driver generation's
//! AV1 *decode* does not share that bug).
//!
//! See [`docs/roadmap.md`](../docs/roadmap.md) for staged status.

#![allow(unsafe_code)]

pub mod av1_params;
pub mod av1_refs;
mod cpu_readback;
mod decoder;
mod decoder_av1;
mod decoder_hevc;
pub mod dpb;
pub mod h264_params;
pub mod h264_slice;
pub mod hevc_params;
pub mod hevc_slice;
pub mod probe;
pub mod session;
mod session_command;
mod session_command_av1;
mod session_command_h264;
mod session_command_hevc;
mod zero_copy;

pub use decoder::VulkanVideoDecoder;

pub use probe::{
    VulkanDecodeCapability, VulkanDecodeProbeError, probe_video_decode_queue_families,
};
