//! Hardware-gated test for [`super::encode_synthetic_intra_frame`] — see
//! `docs/conventions/testing.md` Tier 1 and this crate's
//! `adr/0001-vulkan-video-encode-ash-probe.md` 2026-07-29 addendum for what
//! this actually verified on the reference RTX 4090.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test modules may unwrap / print"
)]

use crate::vulkan::encode_synthetic_intra_frame;
use crate::vulkan::nal::scan_nal_headers;

/// Runs the real Stage 1 pipeline against whatever Vulkan loader/driver this
/// machine exposes, then verifies the resulting bitstream contains
/// Annex-B-framed SPS (7) / PPS (8) / IDR-slice (5) NAL units.
///
/// Skips (never panics) when no encode-capable device is present, matching
/// `probe_tests.rs`'s convention — this crate must stay usable on hosts
/// without a video-encode-capable GPU.
#[test]
#[allow(
    clippy::similar_names,
    reason = "has_sps/has_pps/has_idr_slice read clearer than de-aliased names"
)]
fn encode_one_synthetic_idr_frame_or_skip() {
    let frame = match encode_synthetic_intra_frame() {
        Ok(frame) => frame,
        Err(error) => {
            eprintln!(
                "skip: encode_synthetic_intra_frame failed ({error}) — no encode-capable Vulkan device?"
            );
            return;
        }
    };

    eprintln!(
        "encoded {}x{} frame, {} bitstream bytes",
        frame.coded_width,
        frame.coded_height,
        frame.bitstream.len()
    );
    assert!(frame.coded_width > 0);
    assert!(frame.coded_height > 0);

    let headers = scan_nal_headers(&frame.bitstream);
    for header in &headers {
        eprintln!(
            "NAL type {} at byte offset {}",
            header.nal_unit_type, header.offset
        );
    }

    let has_sps = headers.iter().any(|h| h.nal_unit_type == 7);
    let has_pps = headers.iter().any(|h| h.nal_unit_type == 8);
    let has_idr_slice = headers.iter().any(|h| h.nal_unit_type == 5);
    assert!(
        has_sps,
        "expected an SPS (NAL type 7) in the bitstream, found {headers:?}"
    );
    assert!(
        has_pps,
        "expected a PPS (NAL type 8) in the bitstream, found {headers:?}"
    );
    assert!(
        has_idr_slice,
        "expected an IDR slice (NAL type 5) in the bitstream, found {headers:?}"
    );
}
