//! Unit tests for RIFF/WAVE demux.

#![cfg(test)]
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use super::parse;
use crate::error::Error;
use crate::mux::Muxer;
use crate::types::{SampleFormat, WaveFormat};

fn pcm16_stereo_44k() -> WaveFormat {
    WaveFormat {
        sample_format: SampleFormat::Pcm,
        channels: 2,
        sample_rate: 44_100,
        bits_per_sample: 16,
    }
}

#[test]
fn roundtrips_format_and_samples() {
    let mut mux = Muxer::new(pcm16_stereo_44k());
    mux.push_samples(&[1, 2, 3, 4, 5, 6, 7, 8]);
    let bytes = mux.finish();

    let (format, data) = parse(&bytes).expect("parse");
    assert_eq!(format, pcm16_stereo_44k());
    assert_eq!(&data[..], &[1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn rejects_non_riff_input() {
    let err = parse(b"not a riff file at all").unwrap_err();
    assert!(matches!(err, Error::NotRiffWave));
}

#[test]
fn skips_unknown_chunks_before_data() {
    // RIFF/WAVE with an extra "JUNK" chunk between fmt and data.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&0u32.to_le_bytes()); // patched below
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&8_000u32.to_le_bytes());
    bytes.extend_from_slice(&16_000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"JUNK");
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&[0xaa; 4]);
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&[9, 9, 9]);
    bytes.push(0); // word-align pad for the odd-sized data chunk
    let riff_len = u32::try_from(bytes.len() - 8).unwrap();
    bytes[4..8].copy_from_slice(&riff_len.to_le_bytes());

    let (format, data) = parse(&bytes).expect("parse");
    assert_eq!(format.channels, 1);
    assert_eq!(&data[..], &[9, 9, 9]);
}

#[test]
fn rejects_unsupported_format_tag() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&36u32.to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&6u16.to_le_bytes()); // A-law: unsupported
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&8_000u32.to_le_bytes());
    bytes.extend_from_slice(&8_000u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&8u16.to_le_bytes());

    let err = parse(&bytes).unwrap_err();
    assert!(matches!(err, Error::UnsupportedFormatTag(6)));
}
