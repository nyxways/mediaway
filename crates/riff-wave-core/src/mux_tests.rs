//! Unit tests for RIFF/WAVE mux.

#![cfg(test)]
#![allow(clippy::unwrap_used, reason = "unit tests")]

use super::Muxer;
use crate::types::{SampleFormat, WaveFormat};

fn pcm16_mono_8k() -> WaveFormat {
    WaveFormat {
        sample_format: SampleFormat::Pcm,
        channels: 1,
        sample_rate: 8_000,
        bits_per_sample: 16,
    }
}

#[test]
fn finish_writes_riff_wave_headers() {
    let mut mux = Muxer::new(pcm16_mono_8k());
    mux.push_samples(&[1, 2, 3, 4]);
    let bytes = mux.finish();

    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    assert_eq!(&bytes[12..16], b"fmt ");
    assert_eq!(&bytes[36..40], b"data");
}

#[test]
fn finish_records_correct_sizes() {
    let mut mux = Muxer::new(pcm16_mono_8k());
    mux.push_samples(&[0u8; 10]);
    let bytes = mux.finish();

    let riff_size = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let data_size = u32::from_le_bytes(bytes[40..44].try_into().unwrap());
    assert_eq!(data_size, 10);
    assert_eq!(riff_size as usize, bytes.len() - 8);
}

#[test]
fn finish_pads_odd_sized_data_chunk() {
    let mut mux = Muxer::new(pcm16_mono_8k());
    mux.push_samples(&[1, 2, 3]); // odd length
    let bytes = mux.finish();
    assert_eq!(bytes.len() % 2, 0);
}
