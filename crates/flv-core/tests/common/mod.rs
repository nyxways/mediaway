//! Shared helpers for flv integration / conformance tests.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::missing_const_for_fn,
    clippy::print_stderr,
    unreachable_pub,
    dead_code,
    reason = "test helpers shared across integration binaries"
)]

use flv_core::{Demuxer, TagType};

/// Feed all bytes then drain tags, counting only Audio/Video (exclude `ScriptData`).
/// Returns the audio+video tag count.
pub fn demux_all(bytes: &[u8]) -> usize {
    let mut d = Demuxer::new();
    for chunk in bytes.chunks(64) {
        d.push_bytes(chunk);
    }
    let mut count = 0usize;
    while let Ok(Some(tag)) = d.poll_tag() {
        // Count only Audio/Video tags; exclude ScriptData to match ffprobe's
        // nb_read_packets / nb_frames behavior.
        if tag.tag_type == TagType::Audio || tag.tag_type == TagType::Video {
            count += 1;
        }
    }
    count
}
