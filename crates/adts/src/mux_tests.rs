//! Unit tests for ADTS mux.

#![cfg(test)]
#![allow(clippy::unwrap_used, reason = "unit tests")]

use super::Muxer;
use crate::types::{AacProfile, AdtsConfig};

fn lc_stereo_48k() -> AdtsConfig {
    AdtsConfig {
        profile: AacProfile::Lc,
        sample_rate: 48_000,
        channels: 2,
    }
}

#[test]
fn new_rejects_non_standard_sample_rate() {
    let config = AdtsConfig {
        profile: AacProfile::Lc,
        sample_rate: 44_099,
        channels: 2,
    };
    assert!(Muxer::new(config).is_err());
}

#[test]
fn write_frame_produces_valid_sync_and_length() {
    let mux = Muxer::new(lc_stereo_48k()).unwrap();
    let mut out = Vec::new();
    mux.write_frame(&[1, 2, 3, 4], &mut out).unwrap();

    assert_eq!(out.len(), 11); // 7-byte header + 4-byte payload
    assert_eq!(out[0], 0xFF);
    assert_eq!(out[1] & 0xF0, 0xF0);
    assert!(out[1] & 0x01 == 1, "protection_absent bit must be set");

    let frame_len = ((usize::from(out[3]) & 0x03) << 11)
        | (usize::from(out[4]) << 3)
        | (usize::from(out[5]) >> 5);
    assert_eq!(frame_len, 11);
}

#[test]
fn write_frame_rejects_oversized_payload() {
    let mux = Muxer::new(lc_stereo_48k()).unwrap();
    let mut out = Vec::new();
    let huge = vec![0u8; 0x1FFF];
    assert!(mux.write_frame(&huge, &mut out).is_err());
}

#[test]
fn write_frame_appends_without_clearing_out() {
    let mux = Muxer::new(lc_stereo_48k()).unwrap();
    let mut out = vec![0xAA, 0xBB];
    mux.write_frame(&[1], &mut out).unwrap();
    assert_eq!(&out[0..2], &[0xAA, 0xBB]);
    assert_eq!(out[2], 0xFF);
}
