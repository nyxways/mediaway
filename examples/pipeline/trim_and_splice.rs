//! Decode → trim → splice → re-encode: a real non-linear edit built from the
//! low-level `VideoDecoder`/`VideoEncoder` traits + `mediaway-container` mux/demux.
//!
//! Encodes two short synthetic clips, decodes each back, drops the first/last
//! frame of each (**trim**), concatenates what's left with renumbered contiguous
//! timestamps (**splice**), re-encodes the result, and writes `out_spliced.mp4`.
//!
//! Run:
//! ```text
//! cargo run --example trim_and_splice
//! ```

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::expect_used,
    reason = "example demonstrates the happy path with console output"
)]

use mediaway_common::{
    Bytes, CodecKind, PixelFormat, Rational, StreamInfo, VideoFrame, VideoFrameStorage,
};
use mediaway_container::mp4::{Demuxer, Muxer};
use mediaway_decoder::{VideoDecoderConfig, VideoOutputPreference};
use mediaway_encoder::{VideoEncoder, VideoEncoderConfig, VideoInputPreference};
use mediaway_pipeline::platform;
use std::fs::File;
use std::io::Write as _;

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;

fn nv12_solid(luma: u8) -> Bytes {
    let mut out = vec![luma; (WIDTH * HEIGHT) as usize];
    out.extend(std::iter::repeat_n(128u8, (WIDTH * HEIGHT / 2) as usize));
    Bytes::from(out)
}

fn frame(pts: i64, luma: u8) -> VideoFrame {
    VideoFrame {
        pts,
        duration: 1,
        width: WIDTH,
        height: HEIGHT,
        format: PixelFormat::Nv12,
        storage: VideoFrameStorage::Cpu {
            data: nv12_solid(luma),
        },
    }
}

/// Encode `frames` and mux into a single-track fMP4 (empty `extra_data`: the muxer
/// derives a proper `avcC` record from the first packet's in-band SPS/PPS).
fn encode_clip(frames: &[VideoFrame]) -> Vec<u8> {
    let cfg = VideoEncoderConfig {
        codec: CodecKind::H264,
        width: WIDTH,
        height: HEIGHT,
        time_base: Rational::new(1, 30),
        bitrate_bps: 2_000_000,
        pixel_format: PixelFormat::Nv12,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
    };
    let mut enc = platform::AutoEncoder::open(&mediaway_encoder::auto::AutoVideoEncodeConfig {
        bitrate_bps: cfg.bitrate_bps,
        ..mediaway_encoder::auto::AutoVideoEncodeConfig::new(
            cfg.codec,
            cfg.width,
            cfg.height,
            cfg.time_base,
        )
    })
    .expect("open encoder");

    for f in frames {
        enc.push_frame(f).expect("push_frame");
    }
    enc.flush().expect("flush");
    let mut packets = Vec::new();
    while let Some(p) = enc.poll_packet().expect("poll_packet") {
        packets.push(p);
    }

    let mut track_info = enc.stream_info().clone();
    if let StreamInfo::Video { extra_data, .. } = &mut track_info {
        *extra_data = Bytes::new();
    }
    let mut open = Muxer::new();
    let track_id = open.add_track(track_info).expect("add track");
    let mut mux = open.begin();
    for mut p in packets {
        p.stream_id = track_id;
        mux.push_packet(&p).expect("mux push_packet");
    }
    mux.flush();
    let mut bytes = Vec::new();
    mux.poll_bytes(&mut bytes);
    bytes
}

/// Demux + decode `fmp4_bytes` (H.264 CPU path) into owned frames.
fn decode_clip(fmp4_bytes: &[u8]) -> Vec<VideoFrame> {
    let mut demux = Demuxer::new();
    demux.push_bytes(fmp4_bytes);
    let StreamInfo::Video {
        time_base,
        extra_data,
        ..
    } = demux.streams().first().expect("one video stream").clone()
    else {
        unreachable!("demuxed stream is always Video here")
    };

    let mut dec = platform::AutoDecoder::open(&VideoDecoderConfig {
        codec: CodecKind::H264,
        width: WIDTH,
        height: HEIGHT,
        time_base,
        pixel_format: PixelFormat::Nv12,
        output: VideoOutputPreference::CpuFramesOk,
        gpu_device: None,
        extra_data,
    })
    .expect("open decoder");

    let mut out = Vec::new();
    while let Some(packet) = demux.poll_packet() {
        dec.push_packet(&packet).expect("push_packet");
        while let Some(f) = dec.poll_frame().expect("poll_frame") {
            out.push(f);
        }
    }
    dec.flush().expect("flush");
    while let Some(f) = dec.poll_frame().expect("poll_frame") {
        out.push(f);
    }
    out
}

fn main() {
    // Clip 1: 6 frames stepping luma 10..110; clip 2: 6 frames stepping 130..230.
    let clip_1: Vec<VideoFrame> = (0..6i64)
        .map(|i| frame(i, 10 + u8::try_from(i).expect("small index") * 20))
        .collect();
    let clip_2: Vec<VideoFrame> = (0..6i64)
        .map(|i| frame(i, 130 + u8::try_from(i).expect("small index") * 20))
        .collect();

    println!("trim_and_splice: encoding two source clips");
    let clip_1_bytes = encode_clip(&clip_1);
    let clip_2_bytes = encode_clip(&clip_2);

    println!("trim_and_splice: decoding both clips back");
    let decoded_1 = decode_clip(&clip_1_bytes);
    let decoded_2 = decode_clip(&clip_2_bytes);

    // Trim: drop the first and last frame of each clip.
    let trimmed_1 = &decoded_1[1..decoded_1.len() - 1];
    let trimmed_2 = &decoded_2[1..decoded_2.len() - 1];

    // Splice: concatenate the trimmed segments, renumbering to contiguous timestamps.
    let spliced: Vec<VideoFrame> = trimmed_1
        .iter()
        .chain(trimmed_2.iter())
        .enumerate()
        .map(|(i, f)| VideoFrame {
            pts: i64::try_from(i).expect("small index"),
            duration: 1,
            ..f.clone() // clone: owned VideoFrame with adjusted pts/duration for re-encode
        })
        .collect();

    println!(
        "trim_and_splice: re-encoding {} spliced frames",
        spliced.len()
    );
    let output_bytes = encode_clip(&spliced);

    File::create("out_spliced.mp4")
        .and_then(|mut f| f.write_all(&output_bytes))
        .expect("write out_spliced.mp4");

    println!(
        "trim_and_splice: {} frames → out_spliced.mp4 ({} bytes)",
        spliced.len(),
        output_bytes.len()
    );
}
