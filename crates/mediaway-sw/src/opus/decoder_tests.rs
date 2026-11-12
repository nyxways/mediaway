#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss,
    clippy::suboptimal_flops,
    reason = "test module may unwrap; sample indices here are tiny (hundreds of samples) so \
              the f32 cast never loses meaningful precision; the synthetic-voice formulas \
              favor readability over mul_add"
)]

use super::*;
use crate::opus::config::{OpusApplication, OpusEncoderConfig};
use crate::opus::encoder::OpusEncoder;
use mediaway_common::Rational;

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 1;
const FRAME_SAMPLES: usize = 960;

fn encoder_config() -> OpusEncoderConfig {
    OpusEncoderConfig::new(
        SAMPLE_RATE,
        CHANNELS,
        OpusApplication::Voip,
        Rational::new(1, 50),
    )
}

fn decoder_config() -> OpusDecoderConfig {
    OpusDecoderConfig::new(SAMPLE_RATE, CHANNELS, Rational::new(1, 50))
}

fn sine_frame(samples: usize, channels: u16) -> AudioFrame {
    let mut data = Vec::with_capacity(samples * usize::from(channels) * 4);
    for i in 0..samples {
        let t = i as f32 / SAMPLE_RATE as f32;
        let v = (t * 440.0 * std::f32::consts::TAU).sin() * 0.5;
        for _ in 0..channels {
            data.extend_from_slice(&v.to_le_bytes());
        }
    }
    AudioFrame {
        pts: 0,
        duration: samples as u64,
        sample_rate: SAMPLE_RATE,
        channels,
        format: SampleFormat::F32,
        data: Bytes::from(data),
    }
}

#[test]
fn open_succeeds_for_standard_20ms_config() {
    OpusDecoder::open(&decoder_config()).expect("open");
}

#[test]
fn open_rejects_non_legal_frame_duration() {
    let mut cfg = decoder_config();
    cfg.time_base = Rational::new(1, 700);
    let err = OpusDecoder::open(&cfg).unwrap_err();
    assert!(matches!(err, OpusError::InvalidFrameDuration { .. }));
}

#[test]
fn push_packet_decodes_encoder_output_to_expected_sample_count() {
    let mut enc = OpusEncoder::open(&encoder_config()).expect("open encoder");
    let mut dec = OpusDecoder::open(&decoder_config()).expect("open decoder");

    let frame = sine_frame(FRAME_SAMPLES, CHANNELS);
    enc.push_frame(&frame).expect("push_frame");
    let packet = enc.poll_packet().expect("poll_packet").expect("one packet");

    dec.push_packet(&packet).expect("push_packet");
    let decoded = dec.poll_frame().expect("poll_frame").expect("one frame");
    assert_eq!(decoded.format, SampleFormat::F32);
    assert_eq!(
        decoded.data.len(),
        FRAME_SAMPLES * usize::from(CHANNELS) * 4
    );
    assert!(dec.poll_frame().expect("poll_frame").is_none());
}

#[test]
fn push_packet_treats_empty_payload_as_packet_loss_concealment() {
    let mut dec = OpusDecoder::open(&decoder_config()).expect("open");
    let packet = Packet {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: FRAME_SAMPLES as u64,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::new(),
    };
    dec.push_packet(&packet).expect("push_packet (PLC)");
    let decoded = dec.poll_frame().expect("poll_frame").expect("one frame");
    assert_eq!(decoded.data.len() % 4, 0);
}

#[test]
fn push_packet_rejects_after_flush() {
    let mut dec = OpusDecoder::open(&decoder_config()).expect("open");
    dec.flush().expect("flush");
    let packet = Packet {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 0,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::new(),
    };
    let err = dec.push_packet(&packet).unwrap_err();
    assert_eq!(err, OpusError::Closed);
}

#[test]
fn stream_info_reports_opus_codec() {
    let dec = OpusDecoder::open(&decoder_config()).expect("open");
    assert_eq!(dec.stream_info().codec(), CodecKind::Opus);
}

fn encode_voiced(enc: &mut OpusEncoder, index: usize) -> Vec<u8> {
    let tau = std::f32::consts::TAU;
    let pcm: Vec<f32> = (0..FRAME_SAMPLES)
        .map(|i| {
            let t = (index * FRAME_SAMPLES + i) as f32 / SAMPLE_RATE as f32;
            0.3 * (140.0 * tau * t).sin() + 0.15 * (280.0 * tau * t).sin()
        })
        .collect();
    let mut out = [0u8; 1275];
    let len = enc.encode_float_into(&pcm, &mut out).expect("encode");
    out[..len].to_vec()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

#[test]
fn decode_float_into_roundtrips_and_reports_samples() {
    let mut enc = OpusEncoder::open(&encoder_config()).expect("enc");
    let mut dec = OpusDecoder::open(&decoder_config()).expect("dec");
    let mut out = vec![0.0f32; FRAME_SAMPLES];
    let mut energy = 0.0;
    for k in 0..10 {
        let packet = encode_voiced(&mut enc, k);
        assert_eq!(dec.packet_samples(&packet).expect("nb"), FRAME_SAMPLES);
        let n = dec
            .decode_float_into(Some(&packet), &mut out, false)
            .expect("decode");
        assert_eq!(n, FRAME_SAMPLES);
        energy = rms(&out);
    }
    assert!(energy > 0.05, "decoded rms {energy}");
}

#[test]
fn plc_and_fec_recover_a_lost_frame() {
    let mut enc = OpusEncoder::open(&encoder_config()).expect("enc");
    enc.set_bitrate(32_000).expect("bitrate");
    enc.set_inband_fec(true).expect("fec");
    enc.set_packet_loss_percent(20).expect("loss");
    let mut dec = OpusDecoder::open(&decoder_config()).expect("dec");
    let mut out = vec![0.0f32; FRAME_SAMPLES];
    let packets: Vec<Vec<u8>> = (0..12).map(|k| encode_voiced(&mut enc, k)).collect();
    for p in &packets[..8] {
        dec.decode_float_into(Some(p), &mut out, false)
            .expect("decode");
    }
    // Frame 8 is lost. The next packet (9) carries its FEC: recover, then decode 9 itself.
    let n = dec
        .decode_float_into(Some(&packets[9]), &mut out, true)
        .expect("fec");
    assert_eq!(n, FRAME_SAMPLES);
    assert!(rms(&out) > 0.02, "FEC output is silent (rms {})", rms(&out));
    dec.decode_float_into(Some(&packets[9]), &mut out, false)
        .expect("decode 9");

    // PLC with no packet at all keeps the same duration and fades from real signal.
    let n = dec.decode_float_into(None, &mut out, false).expect("plc");
    assert_eq!(n, FRAME_SAMPLES);
    assert!(rms(&out) > 0.005, "PLC output is silent");
}

#[test]
fn reset_forgets_history_and_closed_decoder_refuses() {
    let mut dec = OpusDecoder::open(&decoder_config()).expect("dec");
    dec.reset().expect("reset");
    let mut out = vec![0.0f32; FRAME_SAMPLES];
    assert!(dec.packet_samples(&[]).is_err());
    dec.flush().expect("flush");
    assert_eq!(
        dec.decode_float_into(None, &mut out, false).unwrap_err(),
        OpusError::Closed
    );
}
