//! Tests for the public [`QuickSyncVideoEncoder`] wrapper — see
//! `docs/conventions/testing.md` Tier 1. The real hardware encode path
//! itself is exercised more thoroughly in `quicksync_tests.rs` (this file
//! covers the public `open`/`VideoEncoder` trait plumbing on top of it).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test modules may unwrap / print"
)]

use super::*;
use mediaway_common::{CodecKind, PixelFormat, Rational, VideoFrameStorage};
use mediaway_encoder::VideoInputPreference;

#[test]
fn open_rejects_zero_copy_gpu_this_stage() {
    let config = VideoEncoderConfig {
        codec: CodecKind::H264,
        width: 176,
        height: 144,
        time_base: Rational::new(1, 30),
        bitrate_bps: 0,
        pixel_format: PixelFormat::Nv12,
        input: VideoInputPreference::ZeroCopyGpu,
        gpu_device: None,
    };
    // Real oneVPL Zero-Copy D3D11 surfaces are deferred (adr/0001) — every
    // build (with or without a real oneVPL runtime) must reject this input
    // path honestly, never silently fall back to CPU upload.
    assert!(matches!(
        QuickSyncVideoEncoder::open(&config),
        Err(EncodeError::Unsupported)
    ));
}

/// Opens a real oneVPL session through the public `QuickSyncVideoEncoder`
/// wrapper (not the crate-internal `QuickSyncSession` directly), pushes a
/// couple of frames, and confirms at least one real packet comes back.
/// Skips (does not fail) when no oneVPL implementation is available.
#[test]
fn public_api_real_encode_or_skips() {
    let width = 176u32;
    let height = 144u32;
    let config = VideoEncoderConfig {
        codec: CodecKind::H264,
        width,
        height,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
    };

    let mut encoder = match QuickSyncVideoEncoder::open(&config) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: QuickSyncVideoEncoder::open failed ({e:?}) — no oneVPL runtime?");
            return;
        }
    };

    let plane = (width as usize) * (height as usize);
    let mut data = vec![0u8; plane + plane / 2];
    data[plane..].fill(128);

    for i in 0..8i64 {
        let frame = VideoFrame {
            pts: i,
            duration: 1,
            width,
            height,
            format: PixelFormat::Nv12,
            storage: VideoFrameStorage::Cpu {
                data: Bytes::from(data.clone()),
            },
        };
        encoder
            .push_frame(&frame)
            .expect("push_frame should succeed against real hardware");
    }
    encoder.flush().expect("flush should succeed");

    let mut count = 0usize;
    while let Some(_p) = encoder.poll_packet().expect("poll_packet should not error") {
        count += 1;
    }
    assert!(count > 0, "expected at least one real encoded packet");
    eprintln!("mediaway-encoder-quicksync: public API real encode produced {count} packet(s)");
}
