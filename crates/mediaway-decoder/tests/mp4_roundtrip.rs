//! Encode → **mux to MP4** → demux → decode, for HEVC.
//!
//! The gap this closes, found 2026-09-21 by a consumer trying to play back a recording
//! mediaway itself had written: `tests/cpu_roundtrip.rs` feeds the decoder packets straight
//! from the encoder, which are Annex-B, and that path worked. Real playback reads packets back
//! out of a container, where they are length-prefixed (`hvcC`) — and *that* path decoded
//! **zero frames**, with every packet accepted and a clean drain. A container in the middle is
//! the whole difference, so a test without one could not have caught it.
//!
//! Skips (does not fail) when this machine has no HEVC encoder MFT, in the same honest shape as
//! the other hardware-gated tests here.

#![cfg(all(windows, feature = "video"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::panic,
    reason = "integration test"
)]

use mediaway_common::{
    Bytes, CodecKind, PixelFormat, Rational, StreamInfo, VideoFrame, VideoFrameStorage,
};
use mediaway_container::mp4;
use mediaway_decoder::windows::WindowsVideoDecoder;
use mediaway_decoder::{VideoDecoder, VideoDecoderConfig, VideoOutputPreference};
use mediaway_encoder::windows::WindowsVideoEncoder;
use mediaway_encoder::{VideoEncoder, VideoEncoderConfig, VideoInputPreference};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;
const FRAMES: u32 = 8;

/// A moving gradient, so a decoded frame can be told apart from a blank one.
fn gradient_nv12(step: u32) -> Bytes {
    let (w, h) = (WIDTH as usize, HEIGHT as usize);
    let mut data = Vec::with_capacity(w * h * 3 / 2);
    let shift = step as usize * 8;
    for y in 0..h {
        for x in 0..w {
            // Luma is limited range, so the values stay inside 16..=235 and the `u8` fits.
            let value = (x + y + shift) % 220 + 16;
            data.push(u8::try_from(value).unwrap_or(u8::MAX));
        }
    }
    data.extend(std::iter::repeat_n(128u8, w * h / 2));
    Bytes::from(data)
}

#[test]
fn hevc_survives_a_trip_through_an_mp4() {
    let Some((packets, stream)) = encode_hevc() else {
        return;
    };

    // --- mux -------------------------------------------------------------------------------
    let mut muxer = mp4::Muxer::new();
    let track = muxer.add_track(stream).expect("register the track");
    let mut muxer = muxer.begin();
    let mut file = Vec::new();
    for packet in &packets {
        let mut packet = packet.clone();
        packet.stream_id = track;
        muxer.push_packet(&packet).expect("mux a packet");
        muxer.poll_bytes(&mut file);
    }
    muxer.flush();
    muxer.poll_bytes(&mut file);
    assert!(file.len() > 1024, "an MP4 with {} packets", packets.len());

    // --- demux -----------------------------------------------------------------------------
    let mut demuxer = mp4::Demuxer::new();
    demuxer.push_bytes(&file);
    let demuxed_stream = demuxer
        .streams()
        .iter()
        .find(|info| info.codec() == CodecKind::Hevc)
        .cloned()
        .expect("the muxed file has an HEVC track");
    let mut read_back = Vec::new();
    while let Some(packet) = demuxer.poll_packet() {
        read_back.push(packet);
    }
    assert_eq!(
        read_back.len(),
        packets.len(),
        "every muxed packet comes back out"
    );
    assert!(
        !iso_bmff::bitstream::hevc::is_annex_b(&read_back[0].payload),
        "MP4 samples are length-prefixed — if this ever changes, the bug this test exists for \
         cannot happen, and the test is measuring nothing"
    );

    // --- decode ----------------------------------------------------------------------------
    let StreamInfo::Video {
        time_base,
        extra_data,
        ..
    } = demuxed_stream
    else {
        panic!("the HEVC track is a video track");
    };
    let mut decoder = WindowsVideoDecoder::open(&VideoDecoderConfig {
        codec: CodecKind::Hevc,
        width: WIDTH,
        height: HEIGHT,
        time_base,
        pixel_format: PixelFormat::Nv12,
        output: VideoOutputPreference::CpuFramesOk,
        gpu_device: None,
        extra_data,
    })
    .expect("open the decoder for a stream this workspace just wrote");

    let mut frames = 0;
    let mut first: Option<VideoFrame> = None;
    for packet in &read_back {
        decoder
            .push_packet(packet)
            .expect("decode a demuxed packet");
        while let Some(frame) = decoder.poll_frame().expect("poll") {
            frames += 1;
            first.get_or_insert(frame);
        }
    }
    decoder.flush().expect("flush");
    while let Some(frame) = decoder.poll_frame().expect("drain") {
        frames += 1;
        first.get_or_insert(frame);
    }

    assert!(
        frames > 0,
        "decoded nothing from a file this workspace wrote — the decoder took every packet and \
         produced no frame, which is what an unconverted hvcC bitstream looks like"
    );
    let frame = first.expect("frames > 0 implies a first frame");
    assert!(frame.width >= WIDTH && frame.height >= HEIGHT, "{frame:?}");
    let VideoFrameStorage::Cpu { data } = &frame.storage else {
        panic!("CpuFramesOk was asked for");
    };
    let luma = &data[..(frame.width * frame.height) as usize];
    assert!(
        luma.iter().any(|&value| value != luma[0]),
        "a decoded gradient that is one flat value is a decode that silently failed"
    );
}

/// Encode a few real HEVC frames, or `None` when this machine has no HEVC encoder MFT.
fn encode_hevc() -> Option<(Vec<mediaway_common::Packet>, StreamInfo)> {
    let mut encoder = match WindowsVideoEncoder::open(&VideoEncoderConfig {
        codec: CodecKind::Hevc,
        width: WIDTH,
        height: HEIGHT,
        time_base: Rational::new(1, 30),
        bitrate_bps: 1_000_000,
        pixel_format: PixelFormat::Nv12,
        color_range: mediaway_common::ColorRange::Video,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
        gop_size: 4,
        rate_control: None,
        intra_refresh_period: None,
    }) {
        Ok(encoder) => encoder,
        Err(e) => {
            eprintln!("SKIP: no HEVC encoder on this machine ({e:?})");
            return None;
        }
    };

    for step in 0..FRAMES {
        let frame = VideoFrame {
            pts: i64::from(step),
            duration: 1,
            width: WIDTH,
            height: HEIGHT,
            format: PixelFormat::Nv12,
            storage: VideoFrameStorage::Cpu {
                data: gradient_nv12(step),
            },
        };
        if let Err(e) = encoder.push_frame(&frame) {
            eprintln!("SKIP: HEVC encode failed ({e:?})");
            return None;
        }
    }
    encoder.flush().expect("encoder flush");
    let mut packets = Vec::new();
    while let Some(packet) = encoder.poll_packet().expect("encoder poll") {
        packets.push(packet);
    }
    if packets.is_empty() {
        eprintln!("SKIP: HEVC encoder produced no packets");
        return None;
    }
    let stream = encoder.stream_info().clone();
    Some((packets, stream))
}
