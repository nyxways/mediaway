//! Real-macOS tests for the `AudioConverter` AAC encoder's stream configuration.
//!
//! The Apple audio backends had no tests until the v0.2.0 release pipeline's RC gate found the
//! decoder unable to open on a real `macos-14` runner. Running the decoder's round trip then showed
//! what this encoder exposes as `extra_data`: Core Audio's ES descriptor, not the bare
//! `AudioSpecificConfig` the stream is documented to carry.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::cast_precision_loss,
    reason = "real-hardware test that reports what the OS answers"
)]

use super::asc_from_cookie;

/// The cookie a real `macos-14` runner's Core Audio returned for AAC-LC, 44.1 kHz, stereo,
/// byte for byte (the extended `80 80 80` length prefixes are Core Audio's own).
const REAL_COOKIE: [u8; 39] = [
    0x03, 0x80, 0x80, 0x80, 0x22, 0x00, 0x00, 0x00, 0x04, 0x80, 0x80, 0x80, 0x14, 0x40, 0x14, 0x00,
    0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0xf4, 0x00, 0x05, 0x80, 0x80, 0x80, 0x02, 0x12,
    0x10, 0x06, 0x80, 0x80, 0x80, 0x01, 0x02,
];

#[test]
fn the_cookie_core_audio_really_returned_yields_the_bare_asc() {
    assert_eq!(asc_from_cookie(&REAL_COOKIE), Some(&[0x12, 0x10][..]));
}

#[test]
fn a_minimal_single_byte_length_descriptor_yields_the_bare_asc() {
    let cookie = [
        0x03, 0x19, 0, 0, 0, 0x04, 0x11, 0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x05, 0x02,
        0x11, 0x90, 0x06, 0x01, 0x02,
    ];
    assert_eq!(asc_from_cookie(&cookie), Some(&[0x11, 0x90][..]));
}

#[test]
fn a_bare_asc_and_malformed_input_are_left_alone() {
    // Not an ES descriptor: the caller keeps the bytes as they are.
    assert_eq!(asc_from_cookie(&[0x12, 0x10]), None);
    assert_eq!(asc_from_cookie(&[]), None);
    // An ES descriptor cut short must not panic or read past the end.
    for cut in 0..REAL_COOKIE.len() {
        let _ = asc_from_cookie(&REAL_COOKIE[..cut]);
    }
}

/// The encoder as it runs: after real encoding, `extra_data` is the bare two-byte AAC-LC ASC.
#[test]
fn the_encoder_exposes_the_bare_asc_as_extra_data() {
    use crate::apple::AppleAudioEncoder;
    use crate::{AudioEncoder, AudioEncoderConfig};
    use mediaway_common::{AudioFrame, Bytes, CodecKind, Rational, SampleFormat, StreamInfo};

    const RATE: u32 = 44_100;
    const FRAME_SAMPLES: usize = 1024;
    let config = AudioEncoderConfig {
        codec: CodecKind::Aac,
        sample_rate: RATE,
        channels: 2,
        sample_format: SampleFormat::F32,
        time_base: Rational::new(1, RATE),
        bitrate_bps: 0,
    };
    let mut encoder = AppleAudioEncoder::open(&config).expect("the Apple AAC encoder opens");
    for i in 0..12usize {
        let mut pcm = Vec::with_capacity(FRAME_SAMPLES * 8);
        for s in 0..FRAME_SAMPLES {
            let t = (i * FRAME_SAMPLES + s) as f32 / RATE as f32;
            let v = (t * 440.0 * std::f32::consts::TAU).sin();
            pcm.extend_from_slice(&v.to_le_bytes());
            pcm.extend_from_slice(&v.to_le_bytes());
        }
        encoder
            .push_frame(&AudioFrame {
                pts: i64::try_from(i * FRAME_SAMPLES).unwrap(),
                duration: FRAME_SAMPLES as u64,
                sample_rate: RATE,
                channels: 2,
                format: SampleFormat::F32,
                data: Bytes::from(pcm),
            })
            .expect("push_frame");
    }
    encoder.flush().expect("flush");
    while encoder.poll_packet().expect("poll_packet").is_some() {}

    let StreamInfo::Audio { extra_data, .. } = encoder.stream_info() else {
        unreachable!("an AAC encoder reports an audio stream");
    };
    eprintln!("extra_data: {extra_data:02x?}");
    assert_eq!(
        &extra_data[..],
        &[0x12, 0x10],
        "the AudioSpecificConfig, not the ES descriptor"
    );
}
