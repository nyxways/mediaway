#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test module may unwrap"
)]
#![allow(
    clippy::float_cmp,
    reason = "timestamps/samples round-trip exact f64/f32 literals through a plain getter, no arithmetic"
)]

use super::*;

#[test]
fn chunk_count_matches_input_length() {
    let data = DecodedAudioData::new(
        vec![0.0, 20_000.0],
        vec![960, 960],
        vec![2, 2],
        vec![vec![0.0; 1920], vec![0.0; 1920]],
    );
    assert_eq!(data.chunk_count(), 2);
}

#[test]
fn fields_read_back_by_index() {
    let data = DecodedAudioData::new(
        vec![0.0, 20_000.0],
        vec![480, 960],
        vec![1, 2],
        vec![vec![0.5, -0.5], vec![0.25, 0.25, -0.25, -0.25]],
    );
    assert_eq!(data.timestamp_us(0), 0.0);
    assert_eq!(data.timestamp_us(1), 20_000.0);
    assert_eq!(data.sample_count(0), 480);
    assert_eq!(data.sample_count(1), 960);
    assert_eq!(data.channel_count(0), 1);
    assert_eq!(data.channel_count(1), 2);
    assert_eq!(data.samples(0), vec![0.5, -0.5]);
    assert_eq!(data.samples(1), vec![0.25, 0.25, -0.25, -0.25]);
}

#[test]
fn empty_data_has_zero_count() {
    let data = DecodedAudioData::new(Vec::new(), Vec::new(), Vec::new(), Vec::new());
    assert_eq!(data.chunk_count(), 0);
}
