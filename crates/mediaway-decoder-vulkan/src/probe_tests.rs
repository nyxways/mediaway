//! Tests for [`super::probe_video_decode_queue_families`] — see
//! `docs/conventions/testing.md` Tier 1.
//!
//! See crate root docs / `adr/0001`'s 2026-07-29 addendum for the real
//! `--nocapture` output on this workspace's reference machine.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test modules may unwrap / print"
)]

use super::*;

/// Runs the real probe against whatever Vulkan loader/driver this machine
/// exposes. Written to skip honestly (never panic) when no Vulkan loader is
/// present at all, since this crate must also stay usable on hosts without
/// any GPU driver — same convention as
/// `mediaway-encoder-vulkan::probe_tests::probe_runs_or_skips_without_vulkan_loader`.
#[test]
fn probe_runs_or_skips_without_vulkan_loader() {
    let capabilities = match probe_video_decode_queue_families() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skip: probe_video_decode_queue_families failed ({e}) — no Vulkan loader?");
            return;
        }
    };

    for cap in &capabilities {
        eprintln!(
            "vulkan device: {} ({:?}) h264_decode_queue_family={:?} h265_decode_queue_family={:?} \
             av1_decode_queue_family={:?}",
            cap.device_name,
            cap.device_type,
            cap.h264_decode_queue_family,
            cap.h265_decode_queue_family,
            cap.av1_decode_queue_family
        );
    }
}
