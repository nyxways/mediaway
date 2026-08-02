#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use mediaway_common::{AudioFrame, Bytes, CodecKind, Rational, SampleFormat, StreamInfo};
use mediaway_device::CaptureError;

use super::{
    SharedQueue, WasapiSession, WindowsWasapiCapture, combine_endpoint_and_interface_names,
    copy_pcm_buffer,
};

/// A capture session with no real worker thread — `worker: None` means
/// `close()` has nothing to join, so this is safe to construct and drive
/// without touching WASAPI/COM at all (mirrors
/// `wasapi_playback_tests.rs::fake_session`).
fn fake_session() -> WindowsWasapiCapture {
    let queue = Arc::new(SharedQueue {
        frames: Mutex::new(VecDeque::new()),
        stop: AtomicBool::new(false),
        device_lost: AtomicBool::new(false),
    });
    let stream_info = StreamInfo::Audio {
        id: 0,
        codec: CodecKind::RawAudio,
        time_base: Rational::new(1, 48_000),
        extra_data: Bytes::new(),
        sample_rate: 48_000,
        channels: 2,
    };
    WindowsWasapiCapture {
        inner: Some(WasapiSession {
            stream_info,
            queue,
            worker: None,
        }),
    }
}

fn pcm_frame() -> AudioFrame {
    AudioFrame {
        pts: 0,
        duration: 1,
        sample_rate: 48_000,
        channels: 2,
        format: SampleFormat::F32,
        data: Bytes::from(vec![0u8; 8]),
    }
}

/// Before `pump_capture_loop` ever sets `device_lost`, an empty queue is
/// just "no frame yet" — not an error (ADR-0005's `DeviceLost` must not fire
/// on ordinary idle periods).
#[test]
fn poll_frame_on_empty_queue_without_device_lost_is_ok_none() {
    let mut capture = fake_session();
    assert_eq!(capture.poll_frame(), Ok(None));
}

/// Once `pump_capture_loop` marks `device_lost`, `poll_frame` must report
/// `CaptureError::DeviceLost` once the queue is drained — the real fix for
/// the previously-silent "worker just stops" gap ADR-0005 names.
#[test]
fn poll_frame_reports_device_lost_after_queue_drains() {
    let mut capture = fake_session();
    if let Some(session) = capture.inner.as_ref() {
        session
            .queue
            .frames
            .lock()
            .expect("lock")
            .push_back(pcm_frame());
        session
            .queue
            .device_lost
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    // The already-buffered frame is delivered first — a lost device must not
    // discard data that was already captured before it disappeared.
    assert!(matches!(capture.poll_frame(), Ok(Some(_))));
    // Only once the queue is empty does the loss surface as an error.
    assert_eq!(capture.poll_frame(), Err(CaptureError::DeviceLost));
    // And it keeps surfacing on every subsequent call — same "session is no
    // longer usable" contract as `CaptureError::Closed`, not a one-shot flag.
    assert_eq!(capture.poll_frame(), Err(CaptureError::DeviceLost));
}

/// `copy_pcm_buffer` replaced a `vec![0u8; len]` zero-fill + `copy_nonoverlapping` pair
/// (two full write passes over the buffer) with a single write into an uninitialized
/// allocation (one pass). This test drives it with a plain in-memory source instead of a
/// live WASAPI packet — deterministic and hardware-independent — and asserts the result
/// is byte-for-byte identical to the source with no leftover zero-fill artifacts, which
/// would catch a capacity/`set_len` mistake in the single-pass rewrite.
#[test]
fn copy_pcm_buffer_reproduces_source_bytes_exactly() {
    // A non-zero, non-repeating pattern so a truncated copy or stale zero-fill would
    // be visible instead of accidentally matching.
    let src: Vec<u8> = (0u32..2048)
        .map(|i| u8::try_from(i % 251).expect("i % 251 fits in u8"))
        .collect();

    // SAFETY: `src` is a live, fully-initialized `Vec<u8>` for the duration of the call.
    let copied = unsafe { copy_pcm_buffer(src.as_ptr(), src.len()) };

    assert_eq!(copied.len(), src.len());
    assert_eq!(copied, src);
}

#[test]
fn copy_pcm_buffer_handles_empty_packet() {
    let src: Vec<u8> = Vec::new();

    // SAFETY: zero-length read from a valid (if empty) pointer.
    let copied = unsafe { copy_pcm_buffer(src.as_ptr(), 0) };

    assert!(copied.is_empty());
}

/// Regression guard sized like a realistic WASAPI period: 480 frames (10ms @ 48kHz)
/// stereo `f32` = 480 * 2 * 4 bytes, matching the `samples * 4` math in
/// `pump_capture_loop`.
#[test]
fn copy_pcm_buffer_matches_typical_period_size() {
    let num_frames = 480usize;
    let channels = 2usize;
    let bytes = num_frames * channels * 4;
    let src: Vec<u8> = (0..bytes)
        .map(|i| u8::try_from(i % 256).unwrap_or(0))
        .collect();

    // SAFETY: `src` is valid for `src.len()` reads.
    let copied = unsafe { copy_pcm_buffer(src.as_ptr(), src.len()) };

    assert_eq!(copied.len(), bytes);
    assert_eq!(copied, src);
}

/// Regression test for a real bug found this session on a Korean-locale
/// Windows machine: `PKEY_Device_FriendlyName` here already embeds the
/// driver name (`"스테레오 믹스 (Realtek(R) Audio)"`), and a naive
/// "always append `PKEY_DeviceInterface_FriendlyName`" combine step
/// duplicated it (`"... (Realtek(R) Audio) (Realtek(R) Audio)"`). The
/// substring check must skip appending when the interface name is already
/// present in the endpoint name, case-insensitively.
#[test]
fn combine_names_skips_append_when_interface_name_already_embedded() {
    let combined = combine_endpoint_and_interface_names(
        Some("스테레오 믹스 (Realtek(R) Audio)".to_owned()),
        Some("Realtek(R) Audio".to_owned()),
    );
    assert_eq!(
        combined.as_deref(),
        Some("스테레오 믹스 (Realtek(R) Audio)")
    );
}

/// The documented collision this disambiguation exists to fix: a bare
/// generic class name (no driver info at all) gets the interface name
/// appended so two different physical microphones no longer enumerate under
/// the identical string.
#[test]
fn combine_names_appends_interface_name_when_not_already_present() {
    let combined = combine_endpoint_and_interface_names(
        Some("마이크".to_owned()),
        Some("USB Audio Device".to_owned()),
    );
    assert_eq!(combined.as_deref(), Some("마이크 (USB Audio Device)"));
}

#[test]
fn combine_names_case_insensitive_match_still_skips_append() {
    let combined = combine_endpoint_and_interface_names(
        Some("Microphone (REALTEK AUDIO)".to_owned()),
        Some("realtek audio".to_owned()),
    );
    assert_eq!(combined.as_deref(), Some("Microphone (REALTEK AUDIO)"));
}

#[test]
fn combine_names_falls_back_to_whichever_property_is_present() {
    assert_eq!(
        combine_endpoint_and_interface_names(Some("Endpoint Only".to_owned()), None).as_deref(),
        Some("Endpoint Only")
    );
    assert_eq!(
        combine_endpoint_and_interface_names(None, Some("Interface Only".to_owned())).as_deref(),
        Some("Interface Only")
    );
    assert_eq!(combine_endpoint_and_interface_names(None, None), None);
}
