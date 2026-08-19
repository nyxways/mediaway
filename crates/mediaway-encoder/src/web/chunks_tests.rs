#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test module may unwrap"
)]
#![allow(
    clippy::float_cmp,
    reason = "timestamps round-trip exact f64 literals through a plain getter, no arithmetic"
)]

use super::*;

#[test]
fn chunk_count_matches_input_length() {
    let chunks = EncodedVideoChunks::new(
        vec![0.0, 33_333.0],
        vec![true, false],
        vec![vec![1, 2], vec![3, 4]],
        None,
    );
    assert_eq!(chunks.chunk_count(), 2);
}

#[test]
fn timestamp_key_and_data_read_back_by_index() {
    let chunks = EncodedVideoChunks::new(
        vec![0.0, 33_333.0],
        vec![true, false],
        vec![vec![1, 2, 3], vec![4, 5]],
        None,
    );
    assert_eq!(chunks.timestamp_us(0), 0.0);
    assert_eq!(chunks.timestamp_us(1), 33_333.0);
    assert!(chunks.is_key(0));
    assert!(!chunks.is_key(1));
    assert_eq!(chunks.data(0), vec![1, 2, 3]);
    assert_eq!(chunks.data(1), vec![4, 5]);
}

#[test]
fn empty_chunks_has_zero_count() {
    let chunks = EncodedVideoChunks::new(Vec::new(), Vec::new(), Vec::new(), None);
    assert_eq!(chunks.chunk_count(), 0);
}

#[test]
fn description_round_trips_when_present() {
    let chunks = EncodedVideoChunks::new(
        vec![0.0],
        vec![true],
        vec![vec![1, 2, 3]],
        Some(vec![9, 9, 9]),
    );
    assert_eq!(chunks.description(), Some(vec![9, 9, 9]));
}

#[test]
fn description_is_none_when_absent() {
    let chunks = EncodedVideoChunks::new(vec![0.0], vec![true], vec![vec![1, 2, 3]], None);
    assert_eq!(chunks.description(), None);
}

#[test]
fn audio_chunk_count_matches_input_length() {
    let chunks = EncodedAudioChunks::new(vec![0.0, 20_000.0], vec![vec![1, 2], vec![3, 4, 5]]);
    assert_eq!(chunks.chunk_count(), 2);
}

#[test]
fn audio_timestamp_and_data_read_back_by_index() {
    let chunks = EncodedAudioChunks::new(vec![0.0, 20_000.0], vec![vec![1, 2, 3], vec![4, 5]]);
    assert_eq!(chunks.timestamp_us(0), 0.0);
    assert_eq!(chunks.timestamp_us(1), 20_000.0);
    assert_eq!(chunks.data(0), vec![1, 2, 3]);
    assert_eq!(chunks.data(1), vec![4, 5]);
}

#[test]
fn empty_audio_chunks_has_zero_count() {
    let chunks = EncodedAudioChunks::new(Vec::new(), Vec::new());
    assert_eq!(chunks.chunk_count(), 0);
}
