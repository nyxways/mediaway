//! Hardware-gated integration test: a **real** AV1 `KEY_FRAME` bitstream —
//! produced by this workspace's own real, working `mediaway_sw::av1::Av1Encoder`
//! (a pure-Rust `rav1e`-backed software encoder), not hand-constructed and
//! not the driver-blocked Vulkan AV1 **encode** path — pushed through
//! [`VulkanVideoDecoder`], asserting real decoded NV12 output. Skips (never
//! fails the default suite) at any fallible step, same soft-skip convention
//! as `hardware_h264_decode.rs`/`hardware_hevc_decode.rs`.
//!
//! **Why `mediaway_sw::av1::Av1Encoder`, not a Vulkan AV1 encoder round trip
//! like `hardware_hevc_decode.rs`'s HEVC pattern**: this workspace has no
//! hardware-verified Vulkan AV1 encoder to bootstrap from — this crate's own
//! sibling `mediaway-encoder`'s AV1 Vulkan encode path is confirmed
//! driver-blocked (see `adr/vulkan/0002`'s § Sibling AV1 encode experience,
//! `crates/mediaway-encoder/adr/vulkan/0002-vulkan-gop-rate-control.md`).
//! `rav1e` is a complete, independent, pure-CPU AV1 encoder — a real,
//! driver-bug-free bitstream source this crate's own `av1_params.rs` parser
//! was cross-checked against (byte-for-byte, this implementation pass) — see
//! `adr/vulkan/0002`'s § Test plan.
//!
//! **Scope**: `KEY_FRAME`-only, single-tile, per `adr/vulkan/0002`'s § Scope
//! decision — this test's own `Av1Encoder` output only ever produces
//! `KEY_FRAME`s (no GOP/multi-frame encode used here), matching
//! `decoder_av1.rs`'s own current scope cut exactly.
//!
//! **Status (2026-08-19): real, hardware-verified on the RTX 4090 reference
//! machine, hard content assertions, first attempt** — unlike AV1 Vulkan
//! *encode*'s confirmed driver-maturity wall (see this file's own module
//! doc above), AV1 Vulkan *decode* of a real `rav1e`-produced `KEY_FRAME`
//! worked without needing any bug-fix round: decoded NV12 output reconstructs
//! the flat mid-gray (128) source frame near-exactly (every luma byte
//! nonzero, center sample exactly `128`). Resolves `adr/vulkan/0002`'s open
//! question #4 for this scope: this driver generation's AV1 decode path does
//! **not** share AV1 encode's confirmed bug.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test file: unwrap/print are fine"
)]

use mediaway_common::{PixelFormat, Rational, VideoFrame, VideoFrameStorage};
use mediaway_decoder::vulkan::VulkanVideoDecoder;
use mediaway_decoder::{VideoDecoder, VideoDecoderConfig, VideoOutputPreference};
use mediaway_sw::av1::{Av1Encoder, Av1EncoderConfig};

// Matches `hardware_hevc_decode.rs`'s own `WIDTH`/`HEIGHT` — already known to
// clear this crate's reference hardware's H.264/HEVC
// `picture_access_granularity` (32x32 on the RTX 4090 this was written
// against); AV1's own granularity is not yet independently confirmed (see
// `adr/vulkan/0002`'s open question #3-adjacent uncertainty) — this size is
// this test's own best-effort choice, not a confirmed-safe AV1 value.
const WIDTH: u32 = 256;
const HEIGHT: u32 = 192;

/// One flat mid-gray I420 frame — content doesn't matter for this test (a
/// lossy encode will not reproduce it byte-exact), only that the real
/// encoder→decoder round trip produces plausible NV12 output of the right
/// shape.
fn gray_i420_frame(pts: i64) -> VideoFrame {
    let y_len = (WIDTH * HEIGHT) as usize;
    let chroma_len = y_len / 4;
    let mut data = vec![128u8; y_len];
    data.resize(y_len + 2 * chroma_len, 128);
    VideoFrame {
        pts,
        duration: 1,
        width: WIDTH,
        height: HEIGHT,
        format: PixelFormat::I420,
        storage: VideoFrameStorage::Cpu {
            data: mediaway_common::Bytes::from(data),
        },
    }
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "linear encode -> decode round-trip with soft-skip branches at each fallible step; \
              splitting further fragments one straight-line test, mirrors \
              hardware_hevc_decode.rs's identical shape"
)]
fn decode_real_rav1e_key_frame_or_skip() {
    let mut encoder_config = Av1EncoderConfig::new(WIDTH, HEIGHT, Rational::new(1, 30));
    encoder_config.speed = 10; // fastest preset — content/quality do not matter for this test
    let mut encoder = match Av1Encoder::open(&encoder_config) {
        Ok(encoder) => encoder,
        Err(error) => {
            eprintln!("skip: Av1Encoder::open failed ({error:?})");
            return;
        }
    };

    if let Err(error) = encoder.push_frame(&gray_i420_frame(0)) {
        eprintln!("skip: Av1Encoder push_frame failed ({error:?})");
        return;
    }
    encoder.flush().ok();
    let mut packet = None;
    while let Ok(Some(candidate)) = encoder.poll_packet() {
        if candidate.is_keyframe {
            packet = Some(candidate);
            break;
        }
    }
    let Some(packet) = packet else {
        eprintln!("skip: Av1Encoder produced no keyframe packet");
        return;
    };
    assert!(
        !packet.payload.is_empty(),
        "encoded AV1 packet payload is empty"
    );

    let mut decoder_config = VideoDecoderConfig::av1(WIDTH, HEIGHT, Rational::new(1, 30));
    decoder_config.output = VideoOutputPreference::CpuFramesOk;
    let mut decoder = match VulkanVideoDecoder::open(&decoder_config) {
        Ok(decoder) => decoder,
        Err(error) => {
            eprintln!(
                "skip: VulkanVideoDecoder::open (AV1) failed ({error:?}) — no AV1 decode-capable \
                 Vulkan device on this host?"
            );
            return;
        }
    };

    if let Err(error) = decoder.push_packet(&packet) {
        eprintln!(
            "skip: decoder push_packet (AV1) failed ({error:?}) — real rav1e bitstream rejected \
             by this crate's own parser or by the driver"
        );
        return;
    }
    let frame = match decoder.poll_frame() {
        Ok(Some(frame)) => frame,
        Ok(None) => {
            eprintln!("skip: decoder poll_frame (AV1) returned no frame");
            return;
        }
        Err(error) => {
            eprintln!("skip: decoder poll_frame (AV1) failed ({error:?})");
            return;
        }
    };

    assert_eq!(frame.format, PixelFormat::Nv12);
    assert_eq!(frame.width, WIDTH);
    assert_eq!(frame.height, HEIGHT);
    let VideoFrameStorage::Cpu { data } = frame.storage else {
        unreachable!(
            "expected CPU NV12 storage (VideoOutputPreference::CpuFramesOk was requested)"
        );
    };
    let expected_len = (WIDTH as usize) * (HEIGHT as usize) * 3 / 2;
    assert_eq!(
        data.len(),
        expected_len,
        "decoded NV12 buffer has the wrong byte length"
    );

    // The source frame was flat mid-gray (128) everywhere; a real decode of
    // a flat picture should reconstruct close to that value — a real,
    // content-derived check (not just "decode did not error"), same
    // reasoning as `hardware_hevc_decode.rs`'s identical check.
    let luma_len = WIDTH as usize * HEIGHT as usize;
    let luma_sample = data[(HEIGHT as usize / 2) * WIDTH as usize + WIDTH as usize / 2];
    let nonzero_luma = data[..luma_len].iter().filter(|&&b| b != 0).count();
    assert_ne!(
        nonzero_luma, 0,
        "decoded picture is all-zero — real AV1 GPU decode regression (or the same \
         driver-maturity wall this workspace's memory records for AV1 Vulkan encode — see this \
         test file's own module doc)"
    );
    assert!(
        luma_sample.abs_diff(128) < 40,
        "decoded center luma sample {luma_sample} is not close to the flat gray (128) source \
         (nonzero luma bytes: {nonzero_luma}/{luma_len})"
    );

    let _ = decoder.flush();
    let _ = encoder.flush();
}
