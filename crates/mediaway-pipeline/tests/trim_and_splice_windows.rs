//! Real decode → trim → splice → re-encode round trip (Windows, H.264 CPU path).
//!
//! Encodes two synthetic clips (distinct per-frame luma so segment identity/order
//! survives compression), muxes/demuxes/decodes each through `mediaway-container`,
//! drops the first/last frame of each (**trim**), concatenates the remaining
//! frames with renumbered contiguous timestamps (**splice**), re-encodes the
//! result, and decodes the output to verify frame count, order, and content.

#![cfg(windows)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::similar_names,
    reason = "integration test; clip_a/clip_b naming is intentionally parallel"
)]

use mediaway_common::{
    Bytes, CodecKind, PixelFormat, Rational, StreamInfo, VideoFrame, VideoFrameStorage,
};
use mediaway_container::mp4::{Demuxer, Muxer};
use mediaway_decoder::windows::WindowsVideoDecoder;
use mediaway_decoder::{VideoDecoder, VideoDecoderConfig, VideoOutputPreference};
use mediaway_encoder::windows::WindowsVideoEncoder;
use mediaway_encoder::{VideoEncoder, VideoEncoderConfig, VideoInputPreference};
use mediaway_test_media::solid_nv12_bytes;

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;

fn frame(pts: i64, luma: u8) -> VideoFrame {
    VideoFrame {
        pts,
        duration: 1,
        width: WIDTH,
        height: HEIGHT,
        format: PixelFormat::Nv12,
        storage: VideoFrameStorage::Cpu {
            data: Bytes::from(solid_nv12_bytes(WIDTH, HEIGHT, luma, 128, 128)),
        },
    }
}

fn mean_luma(data: &[u8]) -> u8 {
    let luma_plane = &data[..(WIDTH * HEIGHT) as usize];
    let sum: u64 = luma_plane.iter().map(|&b| u64::from(b)).sum();
    u8::try_from(sum / u64::from(WIDTH * HEIGHT)).unwrap_or(u8::MAX)
}

/// Encode `frames` (H.264 CPU path) and mux into a single-track fMP4. `None` on
/// `WindowsVideoEncoder::open` failure (no MF encoder available in this environment).
fn encode_frames(frames: &[VideoFrame]) -> Option<Vec<u8>> {
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
    let mut enc = match WindowsVideoEncoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: WindowsVideoEncoder::open failed ({e:?})");
            return None;
        }
    };
    for f in frames {
        enc.push_frame(f).expect("encoder push_frame");
    }
    enc.flush().expect("encoder flush");

    let mut packets = Vec::new();
    while let Some(p) = enc.poll_packet().expect("encoder poll_packet") {
        packets.push(p);
    }
    assert!(!packets.is_empty(), "expected at least one encoded packet");

    // Leave extra_data empty: the muxer derives a proper avcC record from the first
    // packet's in-band SPS/PPS (`iso_bmff::bitstream::avc::to_avcc`) — passing the
    // encoder's own Annex-B-style sequence header verbatim would be stored as-is
    // instead of a real AVCDecoderConfigurationRecord.
    let mut track_info = enc.stream_info().clone();
    if let StreamInfo::Video { extra_data, .. } = &mut track_info {
        *extra_data = Bytes::new();
    }
    let mut open = Muxer::new();
    let track_id = open.add_track(track_info).expect("add video track");
    let mut mux = open.begin();
    for mut p in packets {
        p.stream_id = track_id;
        mux.push_packet(&p).expect("mux push_packet");
    }
    mux.flush();
    let mut bytes = Vec::new();
    mux.poll_bytes(&mut bytes);
    Some(bytes)
}

/// Demux `fmp4_bytes`, open the H.264 CPU decode path against the demuxed track's
/// extradata, and decode every packet. `None` on `WindowsVideoDecoder::open` failure.
fn decode_fmp4(fmp4_bytes: &[u8]) -> Option<Vec<VideoFrame>> {
    let mut demux = Demuxer::new();
    demux.push_bytes(fmp4_bytes);
    let StreamInfo::Video {
        time_base,
        extra_data,
        ..
    } = demux.streams().first().expect("one video stream").clone()
    else {
        unreachable!("demuxed stream is always Video in this test")
    };

    let cfg = VideoDecoderConfig {
        codec: CodecKind::H264,
        width: WIDTH,
        height: HEIGHT,
        time_base,
        pixel_format: PixelFormat::Nv12,
        output: VideoOutputPreference::CpuFramesOk,
        gpu_device: None,
        extra_data,
    };
    let mut dec = match WindowsVideoDecoder::open(&cfg) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("skip: WindowsVideoDecoder::open (CPU) failed ({e:?})");
            return None;
        }
    };

    let mut out = Vec::new();
    while let Some(packet) = demux.poll_packet() {
        dec.push_packet(&packet).expect("decoder push_packet");
        while let Some(f) = dec.poll_frame().expect("decoder poll_frame") {
            out.push(f);
        }
    }
    dec.flush().expect("decoder flush");
    while let Some(f) = dec.poll_frame().expect("decoder poll_frame") {
        out.push(f);
    }
    Some(out)
}

#[test]
fn decode_trim_splice_reencode_roundtrip() {
    let luma_a: [u8; 6] = [10, 30, 50, 70, 90, 110];
    let luma_b: [u8; 6] = [130, 150, 170, 190, 210, 230];

    let frames_a: Vec<VideoFrame> = luma_a
        .iter()
        .enumerate()
        .map(|(i, &l)| frame(i64::try_from(i).expect("index fits i64"), l))
        .collect();
    let frames_b: Vec<VideoFrame> = luma_b
        .iter()
        .enumerate()
        .map(|(i, &l)| frame(i64::try_from(i).expect("index fits i64"), l))
        .collect();

    let Some(clip_a_bytes) = encode_frames(&frames_a) else {
        return;
    };
    let Some(clip_b_bytes) = encode_frames(&frames_b) else {
        return;
    };

    let Some(decoded_a) = decode_fmp4(&clip_a_bytes) else {
        return;
    };
    let Some(decoded_b) = decode_fmp4(&clip_b_bytes) else {
        return;
    };
    assert_eq!(decoded_a.len(), luma_a.len(), "clip A decoded frame count");
    assert_eq!(decoded_b.len(), luma_b.len(), "clip B decoded frame count");

    // Trim: drop the first and last frame of each clip.
    let trimmed_a = &decoded_a[1..decoded_a.len() - 1];
    let trimmed_b = &decoded_b[1..decoded_b.len() - 1];

    // Splice: concatenate the trimmed segments, renumbering to contiguous timestamps.
    let spliced: Vec<VideoFrame> = trimmed_a
        .iter()
        .chain(trimmed_b.iter())
        .enumerate()
        .map(|(i, f)| VideoFrame {
            pts: i64::try_from(i).expect("index fits i64"),
            duration: 1,
            ..f.clone() // clone: owned VideoFrame with adjusted pts/duration for re-encode
        })
        .collect();

    let Some(output_bytes) = encode_frames(&spliced) else {
        return;
    };
    let Some(output_decoded) = decode_fmp4(&output_bytes) else {
        return;
    };

    let expected_luma: Vec<u8> = luma_a[1..luma_a.len() - 1]
        .iter()
        .chain(luma_b[1..luma_b.len() - 1].iter())
        .copied()
        .collect();
    assert_eq!(
        output_decoded.len(),
        expected_luma.len(),
        "spliced frame count"
    );

    let mut last_pts = i64::MIN;
    for (i, (frame, &expected)) in output_decoded.iter().zip(expected_luma.iter()).enumerate() {
        assert!(
            frame.pts >= last_pts,
            "frame {i}: pts {} not monotonic after previous {last_pts}",
            frame.pts
        );
        last_pts = frame.pts;
        assert!(
            matches!(&frame.storage, VideoFrameStorage::Cpu { .. }),
            "frame {i}: expected CPU storage from CpuFramesOk decode"
        );
        let VideoFrameStorage::Cpu { data } = &frame.storage else {
            continue;
        };
        let luma = mean_luma(data);
        let delta = i32::from(luma) - i32::from(expected);
        assert!(
            delta.abs() <= 20,
            "frame {i}: mean luma {luma} not close to expected {expected}"
        );
    }
}
