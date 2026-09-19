//! Integration: timestamps pushed into a real WMF encoder come back out distinct.
//!
//! `runtime_tests.rs` proves `from_hns(to_hns(t)) == t` in isolation. What it cannot prove is
//! that the round trip a *sample* actually takes — written onto `IMFSample::SetSampleTime`,
//! read back off the output sample — is the same one. This pushes frames through the real MFT
//! and checks the property that was violated: no two packets claiming a single instant.
//!
//! # Why both codecs, and which one catches the defect
//!
//! The truncating `from_hns` broke the two codecs in *different* ways. Both cases were verified
//! to fail against the pre-ADR-0013 code by reinstating the truncation (RTX 4090 host,
//! 2026-09-18):
//!
//! - **HEVC — timestamps collapse.** Its MFT echoes the hns written to it exactly, so 30
//!   submitted ticks came back as **20** distinct timestamps: `0, 0, 1, 3, 3, 4, …`.
//! - **H.264 — presentation before decode.** The inbox MFT reorders (B-frames) and returns
//!   each presentation time about one tick late, as a reorder delay. The values land a hair
//!   *below* whole ticks, so truncation shaved the delay off, and B-frames ended up presented
//!   before they were decoded: `dts 2 > pts 1`. Timestamps stayed distinct, so a
//!   distinctness check alone would not have caught it.
//!
//! An earlier version of this file tested H.264 alone, and credited the result to the wrong
//! mechanism. ADR-0013 § Correction records that.
//!
//! Each case skips rather than fails when the machine has no encoder MFT for it, matching the
//! other Windows integration tests: which encoders are registered is a property of the OS
//! install.

#![cfg(windows)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "integration test"
)]

use mediaway_common::{
    Bytes, CodecKind, Packet, PixelFormat, Rational, VideoFrame, VideoFrameStorage,
};
use mediaway_encoder::windows::WindowsVideoEncoder;
use mediaway_encoder::{VideoEncoder, VideoEncoderConfig, VideoInputPreference};

/// 60 fps. Deliberately not a denominator that divides 10 000 000: at `1/60` only every third
/// tick survived a truncating round trip, so a friendlier timebase would pass either way.
const TIME_BASE: Rational = Rational { num: 1, den: 60 };

/// Enough frames to cross several of the collision points. At `1/60` the old code repeated a
/// timestamp in every three, so 30 HEVC frames came back as 20 distinct timestamps.
const FRAMES: i64 = 30;

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;

/// Encode one frame per tick in `ticks`, or `None` if this machine has no such encoder.
fn encode(codec: CodecKind, ticks: &[i64]) -> Option<Vec<Packet>> {
    let config = VideoEncoderConfig {
        codec,
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
            eprintln!("skip: no {codec:?} encoder on this machine ({e:?})");
            return None;
        }
    };

    let nv12_len = (WIDTH * HEIGHT + WIDTH * HEIGHT / 2) as usize;
    for &pts in ticks {
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

    let mut packets = Vec::new();
    while let Some(packet) = encoder.poll_packet().expect("poll") {
        packets.push(packet);
    }
    assert!(!packets.is_empty(), "expected {codec:?} packets");
    Some(packets)
}

/// The property the defect broke: every frame keeps an instant of its own.
///
/// Presentation timestamps are deliberately *not* checked for monotonicity, nor against the
/// submitted ticks. The H.264 MFT emits in decode order, and returns every presentation time
/// displaced by one tick — 30 frames at ticks 0..29 came back as `1, 3, 2, 5, 4, … 29, 28,
/// 30`. That displacement comes from the MFT itself: its own `MFSampleExtension_DecodeTimestamp`
/// for the first packet is 0 while that packet's sample time is ~1 tick. Asserting either
/// property away would be asserting a misunderstanding of a correct stream.
///
/// What must hold is that the frames stay individually addressable — one instant each, evenly
/// spaced, none lost — and that decode order never runs backwards.
fn assert_timestamps_survive(codec: CodecKind, packets: &[Packet]) {
    let mut sorted: Vec<i64> = packets.iter().map(|p| p.pts).collect();
    sorted.sort_unstable();
    for (offset, &pts) in sorted.iter().enumerate() {
        let expected = sorted[0] + i64::try_from(offset).expect("offset");
        assert_eq!(
            pts,
            expected,
            "{codec:?}: presentation timestamps are not {} consecutive ticks: {sorted:?}",
            packets.len()
        );
    }

    // Already true before ADR-0013, through `drain_output`'s decode-order counter; asserted so
    // the pts fix cannot quietly break it.
    for pair in packets.windows(2) {
        assert!(
            pair[1].dts > pair[0].dts,
            "{codec:?}: decode timestamps move backwards at {} -> {}",
            pair[0].dts,
            pair[1].dts
        );
    }
    for p in packets {
        assert!(
            p.dts <= p.pts,
            "{codec:?}: a packet is decoded after it is presented: dts {} > pts {}",
            p.dts,
            p.pts
        );
    }
}

#[test]
fn hevc_packets_each_carry_their_own_timestamp() {
    if let Some(packets) = encode(CodecKind::Hevc, &constant_rate()) {
        assert_timestamps_survive(CodecKind::Hevc, &packets);
    }
}

#[test]
fn h264_packets_each_carry_their_own_timestamp() {
    if let Some(packets) = encode(CodecKind::H264, &constant_rate()) {
        assert_timestamps_survive(CodecKind::H264, &packets);
    }
}

fn constant_rate() -> Vec<i64> {
    (0..FRAMES).collect()
}

/// Irregular but strictly increasing: gaps of 4 and 2 ticks, the way event-driven screen
/// capture delivers frames when the source is not repainting at a steady rate.
fn variable_rate() -> Vec<i64> {
    (0..FRAMES).map(|i| i * 3 + (i % 2)).collect()
}

/// Regression for mediaway#101. Under a variable frame rate, decode timestamps must follow
/// the real submission times, not count frames. The old decode-order counter produced
/// `0, 1, 2, …` whatever the input was. Because the muxer derives sample durations from dts
/// deltas, that made every sample one tick long, and an 8.7 s screen recording's video track
/// declared a length of 0.3 s.
///
/// Checked on two irregular patterns: the dts must be strictly increasing, span the submitted
/// time, and never exceed the pts.
fn assert_dts_follows_real_time(codec: CodecKind) {
    for ticks in [variable_rate(), irregular_rate()] {
        let Some(packets) = encode(codec, &ticks) else {
            return;
        };
        let dts: Vec<i64> = packets.iter().map(|p| p.dts).collect();
        // Real time, not a frame count: the dts span covers the submitted span. The old counter
        // gave `FRAMES - 1` ticks here whatever the input was.
        let span = dts[dts.len() - 1] - dts[0];
        let submitted = ticks[ticks.len() - 1] - ticks[0];
        assert!(
            span >= submitted - 2,
            "{codec:?}: dts spans {span} ticks for input spanning {submitted}: {dts:?}"
        );
        for pair in dts.windows(2) {
            assert!(pair[1] > pair[0], "{codec:?}: dts not increasing: {dts:?}");
        }
        assert_timestamps_survive_vfr(codec, &packets, &ticks);
    }
}

/// Gaps from 1 to 9 ticks in no regular order: a source that repaints in bursts.
fn irregular_rate() -> Vec<i64> {
    let gaps = [1, 7, 2, 9, 1, 1, 5, 3, 8, 2, 1, 6];
    let mut t = 0;
    (0..FRAMES)
        .map(|i| {
            let now = t;
            t += gaps[usize::try_from(i).expect("index") % gaps.len()];
            now
        })
        .collect()
}

/// Every presentation time is one that was submitted (shifted by the encoder's constant
/// reorder delay, if it has one), and every packet presents no earlier than it decodes.
fn assert_timestamps_survive_vfr(codec: CodecKind, packets: &[Packet], ticks: &[i64]) {
    let mut pts: Vec<i64> = packets.iter().map(|p| p.pts).collect();
    pts.sort_unstable();
    let shift = pts[0] - ticks[0];
    let shifted: Vec<i64> = ticks.iter().map(|t| t + shift).collect();
    assert_eq!(
        pts, shifted,
        "{codec:?}: presentation times lost their spacing"
    );
    for p in packets {
        assert!(p.dts <= p.pts, "{codec:?}: dts {} > pts {}", p.dts, p.pts);
    }
}

#[test]
fn hevc_decode_timestamps_follow_a_variable_frame_rate() {
    assert_dts_follows_real_time(CodecKind::Hevc);
}

#[test]
fn h264_decode_timestamps_follow_a_variable_frame_rate() {
    assert_dts_follows_real_time(CodecKind::H264);
}
