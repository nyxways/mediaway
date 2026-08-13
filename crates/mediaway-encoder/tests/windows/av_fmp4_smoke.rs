//! Integration: WMF H.264 + AAC encode → fMP4 mux → demux round-trip.

#![cfg(windows)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "integration test"
)]

use mediaway_common::{
    AudioFrame, Bytes, CodecKind, Packet, PixelFormat, Rational, SampleFormat, VideoFrame,
    VideoFrameStorage,
};
use mediaway_container::mp4::{Demuxer, Muxer};
use mediaway_encoder::{
    AudioEncoder, AudioEncoderConfig, VideoEncoder, VideoEncoderConfig, VideoInputPreference,
};
use mediaway_encoder_windows::{WindowsAudioEncoder, WindowsVideoEncoder};

fn drain_packets<E: VideoEncoder>(enc: &mut E) -> Vec<Packet> {
    let mut out = Vec::new();
    while let Some(p) = enc.poll_packet().expect("poll") {
        out.push(p);
    }
    out
}

fn drain_audio<E: AudioEncoder>(enc: &mut E) -> Vec<Packet> {
    let mut out = Vec::new();
    while let Some(p) = enc.poll_packet().expect("poll") {
        out.push(p);
    }
    out
}

#[test]
fn av_fmp4_smoke_roundtrip() {
    let vcfg = VideoEncoderConfig {
        codec: CodecKind::H264,
        width: 64,
        height: 64,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        color_range: mediaway_common::ColorRange::Video,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
        gop_size: 1,
        rate_control: None,
    };
    let mut venc = match WindowsVideoEncoder::open(&vcfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: video encoder ({e:?})");
            return;
        }
    };

    let acfg = AudioEncoderConfig {
        codec: CodecKind::Aac,
        sample_rate: 48_000,
        channels: 2,
        sample_format: SampleFormat::S16,
        time_base: Rational::new(1, 48_000),
        bitrate_bps: 128_000,
    };
    let mut aenc = match WindowsAudioEncoder::open(&acfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: audio encoder ({e:?})");
            return;
        }
    };

    let nv12_len = 64 * 64 + 64 * 64 / 2;
    for i in 0..3u64 {
        let frame = VideoFrame {
            pts: i64::try_from(i).expect("pts"),
            duration: 1,
            width: 64,
            height: 64,
            format: PixelFormat::Nv12,
            storage: VideoFrameStorage::Cpu {
                data: Bytes::from(vec![0u8; nv12_len]),
            },
        };
        venc.push_frame(&frame).expect("v push");
    }
    venc.flush().expect("v flush");
    let mut vpackets = drain_packets(&mut venc);
    assert!(!vpackets.is_empty(), "expected H.264 packets");

    let pcm_bytes = 2048 * 2 * 2;
    for i in 0..2u64 {
        let frame = AudioFrame {
            pts: i64::try_from(i * 2048).expect("pts"),
            duration: 2048,
            sample_rate: 48_000,
            channels: 2,
            format: SampleFormat::S16,
            data: Bytes::from(vec![0u8; pcm_bytes]),
        };
        aenc.push_frame(&frame).expect("a push");
    }
    aenc.flush().expect("a flush");
    let mut apackets = drain_audio(&mut aenc);
    assert!(!apackets.is_empty(), "expected AAC packets");

    let vinfo = venc.stream_info().clone().with_id(0);
    let ainfo = aenc.stream_info().clone().with_id(1);

    let mut open = Muxer::with_fragment_batch(2);
    open.add_track(vinfo).expect("video track");
    open.add_track(ainfo).expect("audio track");
    let mut mux = open.begin();

    for p in &mut vpackets {
        p.stream_id = 0;
        mux.push_packet(p).expect("mux v");
    }
    for p in &mut apackets {
        p.stream_id = 1;
        mux.push_packet(p).expect("mux a");
    }
    mux.flush();

    let mut bytes = Vec::new();
    mux.poll_bytes(&mut bytes);
    assert!(bytes.len() > 12);
    assert_eq!(&bytes[4..8], b"ftyp");

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    assert_eq!(demux.streams().len(), 2);

    let mut demuxed = 0usize;
    while demux.poll_packet().is_some() {
        demuxed += 1;
    }
    assert!(
        demuxed >= vpackets.len() + apackets.len(),
        "demuxed {demuxed} packets"
    );
}
