//! Integration: timestamps pushed into a real WMF encoder come back out distinct.
//!
//! `runtime_tests.rs` proves `from_hns(to_hns(t)) == t` in isolation. What it cannot prove is
//! that the round trip a *sample* actually takes — written onto `IMFSample::SetSampleTime`,
//! read back off the output sample — is the same one. This pushes frames through the real MFT
//! and checks the property that was violated: no two packets claiming a single instant.
//!
//! The defect this guards against shipped in every recording made through this path
//! (`mediaway-encoder`'s wiki page, § WMF timestamps), and the existing `av_fmp4_smoke` would
//! not have caught it — three frames is too few for `1/30` to collide.
//!
//! Skips rather than fails when the machine has no H.264 encoder MFT, matching the other
//! Windows integration tests: which encoders are registered is a property of the OS install.

#![cfg(windows)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "integration test"
)]

use mediaway_common::{Bytes, CodecKind, PixelFormat, Rational, VideoFrame, VideoFrameStorage};
use mediaway_encoder::windows::WindowsVideoEncoder;
use mediaway_encoder::{VideoEncoder, VideoEncoderConfig, VideoInputPreference};

/// 60 fps. Deliberately not a denominator that divides 10 000 000: at `1/60` only every third
/// tick survived a truncating round trip, so a friendlier timebase would pass either way.
const TIME_BASE: Rational = Rational { num: 1, den: 60 };

/// Enough frames to cross several of the collision points. At `1/60` the old code repeated a
/// timestamp twice in every three, so 30 frames produced roughly 20 duplicates.
const FRAMES: i64 = 30;

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;

#[test]
fn every_encoded_packet_carries_its_own_timestamp() {
    let config = VideoEncoderConfig {
        codec: CodecKind::H264,
        width: WIDTH,
        height: HEIGHT,
        time_base: TIME_BASE,
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        color_range: mediaway_common::ColorRange::Video,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
        gop_size: 1,
        rate_control: None,
        intra_refresh_period: None,
    };
    let mut encoder = match WindowsVideoEncoder::open(&config) {
        Ok(encoder) => encoder,
        Err(e) => {
            eprintln!("skip: no H.264 encoder on this machine ({e:?})");
            return;
        }
    };

    let nv12_len = (WIDTH * HEIGHT + WIDTH * HEIGHT / 2) as usize;
    for pts in 0..FRAMES {
        let frame = VideoFrame {
            pts,
            duration: 1,
            width: WIDTH,
            height: HEIGHT,
            format: PixelFormat::Nv12,
            storage: VideoFrameStorage::Cpu {
                data: Bytes::from(vec![0u8; nv12_len]),
            },
        };
        encoder.push_frame(&frame).expect("push");
    }
    encoder.flush().expect("flush");

    let mut timestamps = Vec::new();
    let mut decode_timestamps = Vec::new();
    while let Some(packet) = encoder.poll_packet().expect("poll") {
        timestamps.push(packet.pts);
        decode_timestamps.push(packet.dts);
    }
    assert!(!timestamps.is_empty(), "expected H.264 packets");

    // The property ffmpeg actually rejected. Packets arrive in decode order, so their decode
    // timestamps — unlike their presentation ones — must never go backwards. They did, because
    // this path reported `dts = pts` while the MFT was reordering.
    for pair in decode_timestamps.windows(2) {
        assert!(
            pair[1] > pair[0],
            "decode timestamps move backwards: {decode_timestamps:?}"
        );
    }
    for (&pts, &dts) in timestamps.iter().zip(&decode_timestamps) {
        assert!(
            dts <= pts,
            "a packet is decoded after it is presented: dts {dts} > pts {pts}"
        );
    }

    // Presentation timestamps are deliberately *not* checked for monotonicity, and not checked
    // against the submitted ticks either. Measured on this machine, 30 frames pushed at ticks
    // 0..29 came back as pts `1, 3, 2, 5, 4, … 29, 28, 30` over dts `0, 1, 2, … 29`: reordered
    // because the MFT emits B-frames, and displaced by one frame because that is the reorder
    // delay the encoder needs in order for the first decode timestamp to be zero. Both are
    // correct H.264 behaviour, and an assertion that forbade either would be asserting a
    // misunderstanding.
    //
    // What must hold is that the frames stay individually addressable: one instant each, evenly
    // spaced, none lost. That is what the collapsing round trip destroyed.
    let mut sorted = timestamps.clone();
    sorted.sort_unstable();
    for (offset, &pts) in sorted.iter().enumerate() {
        let expected = sorted[0] + i64::try_from(offset).expect("offset");
        assert_eq!(
            pts,
            expected,
            "presentation timestamps are not {} consecutive ticks: {sorted:?}",
            timestamps.len()
        );
    }
}
