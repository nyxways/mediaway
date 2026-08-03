//! Opus encode/decode verification harness — end-to-end proof for the
//! README codec table (Windows Opus encode via mediaway-sw, decode via the
//! inbox WMF decoder MFT).
//!
//!   encode: `cargo run -p mediaway --example opus_check -- encode out.opus`
//!   decode: `cargo run -p mediaway --example opus_check -- decode in.opus.ogg`
//!
//! Encode: 2 s of 440 Hz sine (48 kHz stereo f32) → `WindowsAudioEncoder`
//! (Opus → mediaway-sw) → Ogg mux → file. Decode: Ogg demux → WMF inbox
//! `WmfOpusDecoder` → PCM frame stats.

#![allow(
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::panic,
    clippy::expect_used,
    clippy::items_after_statements,
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss
)]

#[cfg(windows)]
use mediaway_common::StreamInfo;
use mediaway_common::{AudioFrame, CodecKind, Packet, Rational, SampleFormat};
use mediaway_container::ogg;
#[cfg(windows)]
use mediaway_decoder::windows::{OpusDecoderConfig, WmfOpusDecoder};
use mediaway_encoder::windows::WindowsAudioEncoder;
use mediaway_encoder::{AudioEncoder, AudioEncoderConfig};
use std::env;

fn sine_frame(sample_rate: u32, channels: u16, samples: usize, freq: f32) -> AudioFrame {
    let mut pcm: Vec<f32> = Vec::with_capacity(samples * usize::from(channels));
    for i in 0..samples {
        let t = i as f32 / sample_rate as f32;
        let s = (t * freq * std::f32::consts::TAU).sin();
        for _ in 0..channels {
            pcm.push(s);
        }
    }
    let data: Vec<u8> = pcm.iter().flat_map(|f| f.to_le_bytes()).collect();
    AudioFrame {
        pts: 0,
        duration: samples as u64,
        sample_rate,
        channels,
        format: SampleFormat::F32,
        data: data.into(),
    }
}

fn pkt(stream_id: u32, pts: i64, key: bool, payload: &[u8]) -> Packet {
    Packet {
        stream_id,
        pts,
        dts: pts,
        duration: 0,
        is_keyframe: key,
        is_discard: false,
        payload: mediaway_common::Bytes::from(payload.to_vec()),
    }
}

fn encode(path: &str) {
    let cfg = AudioEncoderConfig {
        codec: CodecKind::Opus,
        sample_rate: 48_000,
        channels: 2,
        sample_format: SampleFormat::F32,
        time_base: Rational::new(1, 50),
        bitrate_bps: 96_000,
    };
    let mut enc = WindowsAudioEncoder::open(&cfg).expect("open opus encoder");
    let info = enc.stream_info().clone();
    println!("encoder stream_info: {info:?}");

    let total_frames = 100; // 2 s @ 20 ms
    let samples = (48_000 / 50) as usize;
    let mut packets: Vec<Packet> = Vec::new();
    for i in 0..total_frames {
        let mut f = sine_frame(48_000, 2, samples, 440.0);
        f.pts = i;
        // ogg granule = cumulative sample count; 960 samples/frame @ 48 kHz
        f.duration = samples as u64;
        enc.push_frame(&f).expect("push");
        while let Some(p) = enc.poll_packet().expect("poll") {
            packets.push(p);
        }
    }
    enc.flush().expect("flush");
    while let Some(p) = enc.poll_packet().expect("poll") {
        packets.push(p);
    }
    println!("encoded {} opus packets", packets.len());
    assert!(!packets.is_empty());

    // Ogg mux: OpusHead identification header + comment header + audio.
    let head = b"OpusHead\x01\x02\xbb\x0b\x80\xbb\x00\x00\x00\x00\x00";
    let comment = b"OpusTags\x00\x00\x00\x00\x00\x00\x00\x00";
    let mut mo = ogg::Muxer::new(0x4D57);
    mo.push_packet(&pkt(0, 0, true, head)).unwrap();
    mo.push_packet(&pkt(0, 0, true, comment)).unwrap();
    for (i, p) in packets.iter().enumerate() {
        mo.push_packet(&pkt(
            0,
            (samples as i64) * (i as i64 + 1),
            p.is_keyframe,
            &p.payload,
        ))
        .unwrap();
    }
    mo.flush();
    let mut out = Vec::new();
    mo.poll_bytes(&mut out);
    std::fs::write(path, &out).expect("write ogg");
    println!("wrote {path} ({} bytes)", out.len());
}

#[cfg(windows)]
fn decode(path: &str) {
    let bytes = std::fs::read(path).expect("read input");
    let mut d = ogg::Demuxer::new();
    d.push_bytes(&bytes);
    let mut packets: Vec<Packet> = Vec::new();
    while let Some(p) = d.poll_packet() {
        packets.push(p);
    }
    // The identification header (OpusHead) is consumed into the stream's
    // extra_data by the demuxer — parse rate/channels from it.
    let (sample_rate, channels) = d
        .streams()
        .iter()
        .find_map(|s| match s {
            StreamInfo::Audio { extra_data, .. } if extra_data.starts_with(b"OpusHead") => {
                let p = extra_data.as_ref();
                let rate = u32::from_le_bytes([p[12], p[13], p[14], p[15]]);
                Some((rate, u16::from(p[9])))
            }
            _ => None,
        })
        .unwrap_or((48_000, 2));
    println!(
        "stream: {sample_rate} Hz, {channels} ch, {} packets",
        packets.len()
    );

    let mut dec = WmfOpusDecoder::open(&OpusDecoderConfig::new(sample_rate, channels))
        .expect("open WMF opus decoder");
    let mut frames = 0u64;
    let mut samples_out = 0u64;
    for p in packets.iter().skip(1) {
        if p.payload.starts_with(b"OpusTags") {
            continue; // comment header — not audio
        }
        dec.push_packet(p).expect("push packet");
        while let Some(f) = dec.poll_frame().expect("poll frame") {
            frames += 1;
            samples_out += f.data.len() as u64 / (4 * u64::from(channels));
        }
    }
    dec.flush().expect("flush");
    while let Some(f) = dec.poll_frame().expect("poll frame") {
        frames += 1;
        samples_out += f.data.len() as u64 / (4 * u64::from(channels));
    }
    let secs = samples_out as f64 / f64::from(sample_rate);
    println!("decoded {frames} frames, {samples_out} samples ({secs:.2} s @ {sample_rate} Hz)");
    assert!(samples_out > 0, "no PCM decoded");
}

#[cfg(not(windows))]
fn decode(path: &str) {
    println!("WMF Opus decode is Windows-only — cannot decode {path} here");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(std::string::String::as_str) {
        Some("encode") => encode(args.get(2).map_or("out.opus", String::as_str)),
        Some("decode") => decode(args.get(2).map_or("in.opus", String::as_str)),
        _ => {
            eprintln!("usage: opus_check <encode|decode> <path>");
            std::process::exit(2);
        }
    }
}
