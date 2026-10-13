#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use super::*;
use mediaway_common::{CodecKind, ColorRange, Rational};
use mediaway_encoder::windows::WindowsVideoEncoder;
use mediaway_encoder::{VideoEncoder, VideoEncoderConfig, VideoInputPreference};

/// A size that is not a multiple of 16, as a screen region usually is (Recoway's 724 x 274).
const WIDTH: u32 = 724;
const HEIGHT: u32 = 274;
const FRAMES: i64 = 20;

/// NV12 with a gradient that moves each frame, so every frame is a real picture to decode.
fn moving_nv12(n: i64) -> Bytes {
    let (w, h) = (WIDTH as usize, HEIGHT as usize);
    let shift = usize::try_from(n).unwrap() * 7;
    let mut out = vec![128u8; w * h + w * (h / 2)];
    for y in 0..h {
        for x in 0..w {
            out[y * w + x] = u8::try_from(((x + shift) * 255 / w) % 256).unwrap();
        }
    }
    Bytes::from(out)
}

/// Real finding (2026-10-13): the inbox H.264 decoder signals `MF_E_TRANSFORM_STREAM_CHANGE` when
/// the first frame of a stream whose size is not a multiple of 16 is ready (it pads the output to
/// whole macroblocks). The CPU path used to set the input type again and restart streaming there,
/// which discarded every buffered frame: such a stream decoded to zero frames, flush included.
/// Encodes `FRAMES` frames at 724 x 274 with the CPU encoder and decodes them all back.
#[test]
fn a_size_that_is_not_a_multiple_of_16_decodes_every_frame_on_the_cpu() {
    let mut encoder = match WindowsVideoEncoder::open(&VideoEncoderConfig {
        codec: CodecKind::H264,
        width: WIDTH,
        height: HEIGHT,
        time_base: Rational::new(1, 30),
        bitrate_bps: 1_000_000,
        pixel_format: PixelFormat::Nv12,
        color_range: ColorRange::Video,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
        gop_size: 30,
        rate_control: None,
        intra_refresh_period: None,
    }) {
        Ok(encoder) => encoder,
        Err(e) => {
            eprintln!("skip: no CPU H.264 encoder MFT ({e:?})");
            return;
        }
    };
    let mut packets = Vec::new();
    for n in 0..FRAMES {
        encoder
            .push_frame(&VideoFrame {
                pts: n,
                duration: 1,
                width: WIDTH,
                height: HEIGHT,
                format: PixelFormat::Nv12,
                storage: VideoFrameStorage::Cpu {
                    data: moving_nv12(n),
                },
            })
            .expect("encode");
        while let Some(p) = encoder.poll_packet().expect("poll_packet") {
            packets.push(p);
        }
    }
    encoder.flush().expect("encoder flush");
    while let Some(p) = encoder.poll_packet().expect("poll_packet") {
        packets.push(p);
    }
    let extra_data = encoder.stream_info().extra_data().clone(); // clone: owned snapshot for the decoder config

    let mut decoder = WmfH264Decoder::open(&VideoDecoderConfig {
        codec: CodecKind::H264,
        width: WIDTH,
        height: HEIGHT,
        time_base: Rational::new(1, 30),
        pixel_format: PixelFormat::Nv12,
        output: VideoOutputPreference::CpuFramesOk,
        gpu_device: None,
        extra_data,
    })
    .expect("the CPU H.264 decoder");
    let mut frames = Vec::new();
    for packet in &packets {
        decoder.push_packet(packet).expect("push_packet");
        while let Some(f) = decoder.poll_frame().expect("poll_frame") {
            frames.push(f);
        }
    }
    decoder.flush().expect("flush");
    while let Some(f) = decoder.poll_frame().expect("poll_frame") {
        frames.push(f);
    }
    let mut sent: Vec<i64> = packets.iter().map(|p| p.pts).collect();
    sent.sort_unstable();
    assert_eq!(
        sent.len(),
        usize::try_from(FRAMES).unwrap(),
        "one packet a frame"
    );
    assert_eq!(
        frames.iter().map(|f| f.pts).collect::<Vec<_>>(),
        sent,
        "every frame, in order"
    );
    for f in &frames {
        // The decoder's own padded size: at least the stream's, whole macroblocks.
        assert!(
            f.width >= WIDTH && f.height >= HEIGHT,
            "{}x{}",
            f.width,
            f.height
        );
        let VideoFrameStorage::Cpu { data } = &f.storage else {
            unreachable!("CPU output");
        };
        assert!(data.len() >= (f.width * f.height * 3 / 2) as usize);
    }
}
