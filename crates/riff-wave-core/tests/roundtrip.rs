//! Integration: public API mux → demux round trip.

#![forbid(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration tests may unwrap"
)]

use riff_wave_core::{Muxer, SampleFormat, WaveFormat, parse};

#[test]
fn pcm_roundtrip_via_public_api() {
    let format = WaveFormat {
        sample_format: SampleFormat::Pcm,
        channels: 2,
        sample_rate: 48_000,
        bits_per_sample: 16,
    };
    let mut mux = Muxer::new(format);
    let samples: Vec<u8> = (0..64).collect();
    mux.push_samples(&samples);
    let bytes = mux.finish();

    let (parsed_format, data) = parse(&bytes).expect("parse");
    assert_eq!(parsed_format, format);
    assert_eq!(&data[..], &samples[..]);
}
