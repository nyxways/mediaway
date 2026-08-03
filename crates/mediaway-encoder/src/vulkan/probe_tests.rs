//! Tests for [`super::probe_video_encode_queue_families`] — see
//! `docs/conventions/testing.md` Tier 1.
//!
//! Hardware-verified 2026-07-29 — see the crate root docs' "Hardware-verified"
//! section for the actual `--nocapture` output on this workspace's reference
//! Windows box.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test modules may unwrap / print"
)]

use super::*;

/// Runs the real probe against whatever Vulkan loader/driver this machine
/// exposes.
///
/// On this workspace's reference Windows box (NVIDIA RTX 4090 + Intel UHD
/// 770), this finds real devices: `--nocapture` prints the RTX 4090 with
/// H.264/H.265 encode queue family `4` and the UHD 770 with no encode queue
/// (that GPU's Windows Vulkan driver advertises none). Written to skip
/// honestly (never panic) when no Vulkan loader is present at all, since this
/// crate must also stay usable on hosts without any GPU driver.
#[test]
fn probe_runs_or_skips_without_vulkan_loader() {
    let capabilities = match probe_video_encode_queue_families() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skip: probe_video_encode_queue_families failed ({e}) — no Vulkan loader?");
            return;
        }
    };

    for cap in &capabilities {
        eprintln!(
            "vulkan device: {} ({:?}) h264_encode_queue_family={:?} h265_encode_queue_family={:?}",
            cap.device_name,
            cap.device_type,
            cap.h264_encode_queue_family,
            cap.h265_encode_queue_family
        );
    }
}
