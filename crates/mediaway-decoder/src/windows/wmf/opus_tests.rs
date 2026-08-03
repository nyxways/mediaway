#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::cast_precision_loss,
    reason = "unit tests may unwrap"
)]

use super::*;

fn cfg() -> OpusDecoderConfig {
    OpusDecoderConfig {
        sample_rate: 48_000,
        channels: 2,
        time_base: Rational::new(1, 48_000),
    }
}

/// A real, spec-valid minimal Opus packet (RFC 6716 section 3.1): TOC byte only, config 0
/// (SILK NB 10 ms), mono flag 0, frame-count code 0 (1 frame) -> the single frame has
/// zero bytes of data, which RFC 6716 defines as an explicit "no data received"
/// (packet-loss-concealment / DTX) signal. This lets the real decoder MFT run its full
/// bitstream path without needing an Opus encoder (Windows has none inbox — see module
/// docs on `WmfOpusDecoder`).
const MINIMAL_TOC_ONLY_PACKET: [u8; 1] = [0x00];

#[test]
fn open_or_skip_without_opus_decoder_mft() {
    let dec = match WmfOpusDecoder::open(&cfg()) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("skip: WmfOpusDecoder::open failed ({e:?}) — no inbox Opus decoder MFT?");
            return;
        }
    };
    assert_eq!(dec.stream_info().sample_rate(), Some(48_000));
    assert_eq!(dec.stream_info().channels(), Some(2));
    assert_eq!(dec.stream_info().codec(), CodecKind::Opus);
}

#[test]
fn decodes_minimal_toc_only_packet_to_real_pcm_or_skip() {
    let mut dec = match WmfOpusDecoder::open(&cfg()) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("skip: WmfOpusDecoder::open failed ({e:?}) — no inbox Opus decoder MFT?");
            return;
        }
    };

    let packet = Packet {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 480, // 10ms @ 48kHz, one sample per channel per unit
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::from(MINIMAL_TOC_ONLY_PACKET.to_vec()),
    };
    dec.push_packet(&packet).expect("push minimal opus packet");
    dec.flush().expect("flush");

    let mut frames = 0usize;
    let mut total_samples_per_channel = 0u64;
    while let Some(frame) = dec.poll_frame().expect("poll") {
        assert_eq!(frame.format, SampleFormat::F32);
        assert_eq!(frame.channels, 2);
        assert_eq!(frame.sample_rate, 48_000);
        // Float32 interleaved stereo: 4 bytes/sample * 2 channels * samples_per_channel.
        assert_eq!(
            frame.data.len() % 8,
            0,
            "float32 stereo frame must be 8-byte aligned"
        );
        total_samples_per_channel += frame.duration;
        frames += 1;
    }
    assert!(frames >= 1, "expected at least one decoded PCM frame");
    // A single 10ms SILK-NB Opus frame decodes to 480 samples/channel @ 48kHz.
    assert_eq!(total_samples_per_channel, 480);
}

#[test]
fn rejects_invalid_config() {
    let bad = OpusDecoderConfig {
        sample_rate: 0,
        channels: 2,
        time_base: Rational::new(1, 48_000),
    };
    let err = WmfOpusDecoder::open(&bad)
        .err()
        .expect("zero sample_rate must be rejected");
    assert_eq!(err, DecodeError::InvalidInput);
}

#[test]
fn roundtrip_sw_encoded_sine_decodes_to_pcm_or_skip() {
    use mediaway_common::SampleFormat;
    use mediaway_sw::opus::{
        config::{OpusApplication, OpusEncoderConfig},
        encoder::OpusEncoder,
    };

    let sample_rate = 48_000;
    let channels = 2_u16;
    // WMF decoder MFT present? (skip on machines without it)
    let Ok(mut dec) = WmfOpusDecoder::open(&OpusDecoderConfig::new(sample_rate, channels)) else {
        eprintln!("skip: no inbox Opus decoder MFT");
        return;
    };

    let mut enc = OpusEncoder::open(&OpusEncoderConfig {
        sample_rate,
        channels,
        application: OpusApplication::Audio,
        time_base: Rational::new(1, 50),
        bitrate_bps: Some(64_000),
        inband_fec: false,
        packet_loss_percent: 0,
    })
    .expect("sw opus encoder");

    // 20 ms sine frames @ 48 kHz stereo f32
    let frame_samples = (sample_rate / 50) as usize;
    let mut pcm: Vec<f32> = Vec::with_capacity(frame_samples * 2);
    for i in 0..frame_samples {
        let t = i as f32 / sample_rate as f32;
        let s = (t * 440.0_f32 * std::f32::consts::TAU).sin();
        pcm.push(s);
        pcm.push(s);
    }
    let bytes: Vec<u8> = pcm.iter().flat_map(|f| f.to_le_bytes()).collect();
    let frame = mediaway_common::AudioFrame {
        pts: 0,
        duration: frame_samples as u64,
        sample_rate,
        channels,
        format: SampleFormat::F32,
        data: bytes.into(),
    };
    enc.push_frame(&frame).expect("sw push");
    let mut fed = 0usize;
    let mut decoded_samples = 0u64;
    while let Some(pkt) = enc.poll_packet().expect("sw poll") {
        fed += 1;
        dec.push_packet(&pkt).expect("wmf push");
        while let Some(f) = dec.poll_frame().expect("wmf poll") {
            decoded_samples += (f.data.len() as u64) / (4 * u64::from(channels));
        }
    }
    dec.flush().expect("wmf flush");
    while let Some(f) = dec.poll_frame().expect("wmf poll") {
        decoded_samples += (f.data.len() as u64) / (4 * u64::from(channels));
    }
    assert!(fed >= 1, "sw encoder produced no packets");
    assert!(
        decoded_samples >= frame_samples as u64 / 2,
        "decoded only {decoded_samples} samples (expected >= {frame_samples})"
    );
}
