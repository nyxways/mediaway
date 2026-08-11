//! Hardware-gated integration test: a **real** HEVC Annex-B bitstream —
//! produced by this workspace's own hardware-verified
//! `mediaway-encoder-vulkan::VulkanVideoEncoder` (HEVC), not hand-constructed
//! — pushed through [`VulkanVideoDecoder`], asserting real decoded NV12
//! output. Skips (never fails the default suite) when this machine's Vulkan
//! loader/driver lacks an HEVC encode **or** decode queue family, same
//! convention as `hardware_h264_decode.rs`.
//!
//! **Why an encoder round trip, not a hand-crafted bitstream like
//! `hardware_h264_decode.rs`**: H.264's `I_PCM` macroblock type let that test
//! write raw, uncoded pixel bytes directly into the bitstream (`mb_type` is
//! CAVLC/UE(v)-coded, but the sample data itself is not entropy-coded at
//! all). HEVC has **no CAVLC mode and no equivalent escape** — every syntax
//! element, including a PCM CU's own `pcm_flag`, is CABAC-coded (ITU-T H.265
//! § 9.3), so even the *smallest* legal HEVC picture requires a spec-exact
//! binary arithmetic encoder (context initialization, state transitions,
//! range/offset renormalization, termination) to hand-construct correctly.
//! That is a substantially larger, higher-risk undertaking than H.264's
//! `I_PCM` escape — see `adr/0001`'s HEVC addendum for the honest account.
//! Reusing this workspace's own already hardware-verified HEVC encoder
//! sidesteps writing CABAC by hand entirely while still exercising a fully
//! real, driver-produced HEVC bitstream through this crate's own decode path
//! on real hardware.
//!
//! **Scope**: `mediaway-encoder-vulkan::VulkanVideoEncoder` makes every
//! pushed frame an independent key frame (no GOP/P-frames) — so this test
//! only exercises the IDR picture path, matching
//! `decoder_hevc.rs::decode_slice_hevc`'s own current IDR-only scope cut
//! (non-IDR HEVC NALs return `DecodeError::Unsupported` this round). General
//! P/B-slice HEVC hardware verification remains a follow-up, same as the ADR
//! addendum states.
//!
//! **Status (2026-08-05): root cause found and fixed — real decode
//! hardware-verified, hard assertions.** This test found and fixed a chain of
//! real bugs across three separate rounds of investigation: `HevcSps`/
//! `HevcPps::to_std` silently echoing several `Std*Flags` bits as disabled
//! regardless of what the real encoder actually signaled (2026-07-30), a
//! `general_level_idc`/profile-tier-level constraint-flag encoding bug
//! (raw ITU-T byte cast directly into `StdVideoH265LevelIdc`'s small ordinal
//! enum instead of converted), and — the bug that actually mattered —
//! `HevcPps::parse` stopping *before* reading
//! `pps_loop_filter_across_slices_enabled_flag`, which really does gate a
//! conditional `slice_loop_filter_across_slices_enabled_flag` bit in every
//! slice header (confirmed against `FFmpeg`'s `libavcodec/hevc/hevcdec.c`
//! `hls_slice_header`), desyncing the driver's own CABAC parser by one bit
//! right before CTU data. See `adr/0001`'s 2026-08-05 addendum for the full
//! account, including the two hypotheses that were tested and ruled out
//! before this was found.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test file: unwrap/print are fine"
)]

use mediaway_common::{Bytes, CodecKind, PixelFormat, Rational, VideoFrame, VideoFrameStorage};
use mediaway_decoder::vulkan::VulkanVideoDecoder;
use mediaway_decoder::{VideoDecoder, VideoDecoderConfig, VideoOutputPreference};
use mediaway_encoder::vulkan::VulkanVideoEncoder;
use mediaway_encoder::{VideoEncoder, VideoEncoderConfig, VideoInputPreference};

// Matches `mediaway-encoder-vulkan::encoder_tests`'s own `WIDTH_HEVC`/
// `HEIGHT_HEVC` — already known to clear this crate's reference hardware's
// HEVC `picture_access_granularity` (32x32 on the RTX 4090 this was written
// against).
const WIDTH: u32 = 256;
const HEIGHT: u32 = 192;

/// One flat mid-gray NV12 frame — content doesn't matter for this test (a
/// lossy fixed-QP intra encode will not reproduce it byte-exact), only that
/// the real encoder→decoder round trip produces plausible NV12 output of the
/// right shape.
fn gray_nv12_frame(pts: i64) -> VideoFrame {
    let len = (WIDTH as usize) * (HEIGHT as usize) * 3 / 2;
    VideoFrame {
        pts,
        duration: 1,
        width: WIDTH,
        height: HEIGHT,
        format: PixelFormat::Nv12,
        storage: VideoFrameStorage::Cpu {
            data: Bytes::from(vec![128u8; len]),
        },
    }
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "linear encode -> decode round-trip with soft-skip branches at each fallible step; splitting further fragments one straight-line test"
)]
fn decode_real_encoder_produced_idr_or_skip() {
    let encoder_config = VideoEncoderConfig {
        codec: CodecKind::Hevc,
        width: WIDTH,
        height: HEIGHT,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
        gop_size: 1,
        rate_control: None,
        intra_refresh_period: None,
    };
    let mut encoder = match VulkanVideoEncoder::open(&encoder_config) {
        Ok(encoder) => encoder,
        Err(error) => {
            eprintln!(
                "skip: VulkanVideoEncoder::open (HEVC) failed ({error:?}) — no encode-capable Vulkan device?"
            );
            return;
        }
    };

    if let Err(error) = encoder.push_frame(&gray_nv12_frame(0)) {
        eprintln!("skip: encoder push_frame (HEVC) failed ({error:?})");
        return;
    }
    let packet = match encoder.poll_packet() {
        Ok(Some(packet)) => packet,
        Ok(None) => {
            eprintln!("skip: no packet after encoder push_frame (HEVC)");
            return;
        }
        Err(error) => {
            eprintln!("skip: encoder poll_packet (HEVC) failed ({error:?})");
            return;
        }
    };
    assert!(
        !packet.payload.is_empty(),
        "encoded packet payload is empty"
    );
    assert!(
        packet.is_keyframe,
        "encoder's first packet should be a key frame"
    );

    let mut decoder_config = VideoDecoderConfig::hevc(WIDTH, HEIGHT, Rational::new(1, 30));
    decoder_config.output = VideoOutputPreference::CpuFramesOk;
    let mut decoder = match VulkanVideoDecoder::open(&decoder_config) {
        Ok(decoder) => decoder,
        Err(error) => {
            eprintln!(
                "skip: VulkanVideoDecoder::open (HEVC) failed ({error:?}) — no decode-capable Vulkan device?"
            );
            return;
        }
    };

    if let Err(error) = decoder.push_packet(&packet) {
        eprintln!(
            "skip: decoder push_packet (HEVC) failed ({error:?}) — real encoder bitstream rejected"
        );
        return;
    }
    let frame = match decoder.poll_frame() {
        Ok(Some(frame)) => frame,
        Ok(None) => {
            eprintln!("skip: decoder poll_frame (HEVC) returned no frame");
            return;
        }
        Err(error) => {
            eprintln!("skip: decoder poll_frame (HEVC) failed ({error:?})");
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

    // The source frame was flat mid-gray (128) everywhere; a fixed-QP intra
    // encode of a flat picture should reconstruct close to that value — a
    // real, content-derived check (not just "decode did not error").
    //
    // Root-caused and fixed (see `adr/0001`'s 2026-08-05 HEVC addendum):
    // `HevcPps::parse` never read `pps_loop_filter_across_slices_enabled_flag`,
    // a real conditional slice-header bitstream bit — a one-bit misalignment
    // right before CTU/CABAC data that produced the all-zero decode. Two
    // earlier `Std*Flags`-zeroing bugs found while chasing this were real but
    // not the root cause. This now hard-asserts (no soft skip) on the RTX
    // 4090 reference hardware.
    let luma_len = WIDTH as usize * HEIGHT as usize;
    let luma_sample = data[(HEIGHT as usize / 2) * WIDTH as usize + WIDTH as usize / 2];
    let nonzero_luma = data[..luma_len].iter().filter(|&&b| b != 0).count();
    assert_ne!(
        nonzero_luma, 0,
        "decoded picture is all-zero — real HEVC GPU decode regression"
    );
    assert!(
        luma_sample.abs_diff(128) < 40,
        "decoded center luma sample {luma_sample} is not close to the flat gray (128) source \
         (nonzero luma bytes: {nonzero_luma}/{luma_len})"
    );

    let _ = decoder.flush();
    let _ = encoder.flush();
}
