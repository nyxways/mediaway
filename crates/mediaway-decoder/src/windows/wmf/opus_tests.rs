#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
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
