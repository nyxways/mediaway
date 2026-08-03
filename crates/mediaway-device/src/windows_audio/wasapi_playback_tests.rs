//! Unit tests for `wasapi_playback.rs` (sibling of the implementation).
//!
//! Most of these build a [`WindowsWasapiPlayback`] session by hand (private
//! fields, no real `IAudioClient`/`IAudioRenderClient`) so queue-full,
//! format-validation, and underrun-counter behavior are exercised
//! deterministically without a real render endpoint. `fill_from_queue` and
//! `read_float_mix` are pure/near-pure and tested directly. Only
//! `open_rejects_wrong_kind_device_id_select` calls the real `open()` entry
//! point — it returns `Unsupported` from `resolve_endpoint`'s `Select::Id`
//! arm before any real endpoint `IAudioClient::Initialize` call is made, so
//! it is safe without a real render device, but still takes
//! `HARDWARE_TEST_LOCK` for consistency with this crate's other
//! `open()`-calling tests.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::audio::{AudioPlayback, AudioPlaybackConfig, PlaybackError};
use mediaway_common::{AudioFrame, Bytes, CodecKind, Rational, SampleFormat, StreamInfo};
use windows::Win32::Media::Audio::WAVEFORMATEX;

use super::{
    PLAYBACK_QUEUE_CAP, PlaybackQueueState, PlaybackSession, PlaybackSharedQueue,
    WindowsWasapiPlayback, fill_from_queue,
};
use crate::windows_audio::wasapi::read_float_mix;

const WAVE_FORMAT_PCM: u16 = 0x0001;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;

fn pcm_frame(sample_rate: u32, channels: u16, byte_pattern_start: u8, len: usize) -> AudioFrame {
    let data: Vec<u8> = (0..len)
        .map(|i| byte_pattern_start.wrapping_add(u8::try_from(i % 251).unwrap_or(0)))
        .collect();
    AudioFrame {
        pts: 0,
        duration: u64::try_from(len).unwrap_or(0),
        sample_rate,
        channels,
        format: SampleFormat::F32,
        data: Bytes::from(data),
    }
}

/// A playback session with no real worker thread — `worker: None` means
/// `close()` has nothing to join, so this is safe to construct and drive
/// without touching WASAPI/COM at all.
fn fake_session(sample_rate: u32, channels: u16) -> WindowsWasapiPlayback {
    let queue = Arc::new(PlaybackSharedQueue {
        state: Mutex::new(PlaybackQueueState {
            frames: VecDeque::new(),
            cursor: 0,
        }),
        stop: AtomicBool::new(false),
        underrun_count: AtomicU64::new(0),
        device_lost: AtomicBool::new(false),
    });
    let stream_info = StreamInfo::Audio {
        id: 0,
        codec: CodecKind::RawAudio,
        time_base: Rational::new(1, sample_rate.max(1)),
        extra_data: Bytes::new(),
        sample_rate,
        channels,
    };
    WindowsWasapiPlayback {
        inner: Some(PlaybackSession {
            stream_info,
            queue,
            worker: None,
        }),
    }
}

#[test]
fn write_frame_rejects_mismatched_sample_rate() {
    let mut playback = fake_session(48_000, 2);
    let mut frame = pcm_frame(48_000, 2, 0, 32);
    frame.sample_rate = 44_100;
    assert_eq!(
        playback.write_frame(frame),
        Err(PlaybackError::InvalidInput)
    );
}

#[test]
fn write_frame_rejects_mismatched_channels() {
    let mut playback = fake_session(48_000, 2);
    let mut frame = pcm_frame(48_000, 2, 0, 32);
    frame.channels = 1;
    assert_eq!(
        playback.write_frame(frame),
        Err(PlaybackError::InvalidInput)
    );
}

#[test]
fn write_frame_accepts_matching_format_until_queue_full() {
    let mut playback = fake_session(48_000, 2);
    for _ in 0..PLAYBACK_QUEUE_CAP {
        let frame = pcm_frame(48_000, 2, 0, 16);
        assert!(playback.write_frame(frame).is_ok());
    }
    let overflow = pcm_frame(48_000, 2, 7, 16);
    let result = playback.write_frame(overflow.clone());
    assert!(
        matches!(&result, Err(PlaybackError::QueueFull(returned)) if *returned == overflow),
        "expected QueueFull(overflow) once the bounded queue is at capacity, got {result:?}"
    );
}

#[test]
fn underrun_count_starts_at_zero_and_reflects_worker_updates() {
    let playback = fake_session(48_000, 2);
    assert_eq!(playback.underrun_count(), 0);
    if let Some(session) = playback.inner.as_ref() {
        session.queue.underrun_count.store(3, Ordering::Relaxed);
    }
    assert_eq!(playback.underrun_count(), 3);
}

#[test]
fn close_without_worker_does_not_block_and_marks_session_closed() {
    let mut playback = fake_session(48_000, 2);
    assert!(playback.close().is_ok());
    assert_eq!(
        playback.write_frame(pcm_frame(48_000, 2, 0, 16)),
        Err(PlaybackError::Closed)
    );
}

#[test]
fn fill_from_queue_reports_full_underrun_on_empty_queue() {
    let mut queue: VecDeque<AudioFrame> = VecDeque::new();
    let mut cursor = 0usize;
    let mut dst = vec![0xAAu8; 32];
    let written = fill_from_queue(&mut queue, &mut cursor, &mut dst);
    assert_eq!(written, 0);
}

#[test]
fn fill_from_queue_reports_partial_underrun_and_leaves_tail_for_caller_to_zero() {
    let mut queue: VecDeque<AudioFrame> = VecDeque::new();
    queue.push_back(pcm_frame(48_000, 2, 1, 16));
    let mut cursor = 0usize;
    let mut dst = vec![0xAAu8; 32];
    let written = fill_from_queue(&mut queue, &mut cursor, &mut dst);
    assert_eq!(written, 16);
    assert!(queue.is_empty());
    assert_eq!(&dst[..16], &pcm_frame(48_000, 2, 1, 16).data[..]);
    // fill_from_queue never memsets — ADR-0005 assigns the explicit zero-fill
    // tail to the render loop (`pump_playback_loop`), not this helper.
    assert!(dst[16..].iter().all(|&b| b == 0xAA));
}

#[test]
fn fill_from_queue_copies_full_period_across_multiple_frames() {
    let mut queue: VecDeque<AudioFrame> = VecDeque::new();
    queue.push_back(pcm_frame(48_000, 2, 1, 8));
    queue.push_back(pcm_frame(48_000, 2, 100, 8));
    queue.push_back(pcm_frame(48_000, 2, 200, 16));
    let mut cursor = 0usize;
    let mut dst = vec![0u8; 24];
    let written = fill_from_queue(&mut queue, &mut cursor, &mut dst);
    assert_eq!(written, 24);
    assert_eq!(
        queue.len(),
        1,
        "third frame partially consumed, still queued"
    );
    assert_eq!(cursor, 8, "8 of the third frame's 16 bytes consumed");
    let expected: Vec<u8> = pcm_frame(48_000, 2, 1, 8)
        .data
        .iter()
        .chain(pcm_frame(48_000, 2, 100, 8).data.iter())
        .chain(pcm_frame(48_000, 2, 200, 16).data[..8].iter())
        .copied()
        .collect();
    assert_eq!(dst, expected);
}

#[test]
fn read_float_mix_accepts_ieee_float_tag() {
    let mut format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_IEEE_FLOAT,
        nChannels: 2,
        nSamplesPerSec: 48_000,
        nAvgBytesPerSec: 48_000 * 2 * 4,
        nBlockAlign: 8,
        wBitsPerSample: 32,
        cbSize: 0,
    };
    let ptr: *mut WAVEFORMATEX = &raw mut format;
    // SAFETY: `ptr` points at a live, fully-initialized `WAVEFORMATEX` for the
    // duration of the call.
    let (sample_rate, channels, valid) = unsafe { read_float_mix(ptr) };
    assert!(valid);
    assert_eq!(sample_rate, 48_000);
    assert_eq!(channels, 2);
}

#[test]
fn read_float_mix_rejects_non_float_tag() {
    let mut format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM,
        nChannels: 2,
        nSamplesPerSec: 48_000,
        nAvgBytesPerSec: 48_000 * 2 * 2,
        nBlockAlign: 4,
        wBitsPerSample: 16,
        cbSize: 0,
    };
    let ptr: *mut WAVEFORMATEX = &raw mut format;
    // SAFETY: `ptr` points at a live, fully-initialized `WAVEFORMATEX` for the
    // duration of the call.
    let (_, _, valid) = unsafe { read_float_mix(ptr) };
    assert!(!valid);
}

#[test]
fn open_rejects_wrong_kind_device_id_select() {
    let _guard = crate::windows_audio::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // A `Select::Id` wrapping a non-WASAPI `DeviceId` (e.g. a DXGI output
    // name) is rejected as `Unsupported` before any endpoint match is
    // attempted — see `wasapi::resolve_endpoint`'s `Select::Id` arm and
    // ADR-0005's "wrong-kind DeviceId" rejection contract.
    let cfg = AudioPlaybackConfig {
        select: crate::Select::Id(crate::DeviceId::from_dxgi_output_device_name(
            "\\\\.\\DISPLAY1",
        )),
        sample_format: SampleFormat::F32,
    };
    let result = WindowsWasapiPlayback::open(&cfg);
    assert!(matches!(result, Err(PlaybackError::Unsupported)));
}
