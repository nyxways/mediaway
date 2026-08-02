#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use super::*;
use mediaway_common::Rational;
use mediaway_device::Select;
use mediaway_device_audio::{AudioCaptureConfig, AudioPlayback, AudioPlaybackConfig};

#[test]
fn open_wasapi_mic_poll_or_skip() {
    let _guard = crate::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let cfg = AudioCaptureConfig::microphone(Rational::new(1, 48_000));
    let mut cap = match WindowsWasapiCapture::open_microphone(&cfg) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skip: WindowsWasapiCapture::open_microphone ({e:?})");
            return;
        }
    };
    let sample_rate = cap
        .stream_info()
        .sample_rate()
        .expect("stream has sample_rate");
    let channels = cap.stream_info().channels().expect("stream has channels");
    assert!(sample_rate > 0);
    assert!(channels > 0);
    std::thread::sleep(std::time::Duration::from_millis(50));
    match cap.poll_frame() {
        Ok(Some(frame)) => {
            assert!(!frame.data.is_empty());
            eprintln!(
                "mic frame samples~={} rate={} ch={}",
                frame.data.len() / 4,
                frame.sample_rate,
                frame.channels
            );
        }
        Ok(None) => eprintln!("mic: no frame yet (ok)"),
        Err(e) => eprintln!("skip: mic poll ({e:?})"),
    }
    cap.close().expect("close");
}

/// Opens the real default render endpoint and lets the worker run a few periods with an
/// **empty** write queue — every period is a full underrun, so WASAPI is told to play
/// `AUDCLNT_BUFFERFLAGS_SILENT` (silence), never caller audio. Verifies the real
/// timer-poll + silence-fill path end to end without writing any audible tone:
/// `underrun_count()` must climb while queued, matching ADR-0005's "render side underrun
/// -> silence, not an error" contract.
#[test]
fn open_wasapi_playback_silence_or_skip() {
    let _guard = crate::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let cfg = AudioPlaybackConfig {
        select: Select::Default,
        sample_format: mediaway_common::SampleFormat::F32,
    };
    let mut playback = match WindowsWasapiPlayback::open(&cfg) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("skip: WindowsWasapiPlayback::open ({e:?})");
            return;
        }
    };
    let sample_rate = playback
        .stream_info()
        .sample_rate()
        .expect("stream has sample_rate");
    let channels = playback
        .stream_info()
        .channels()
        .expect("stream has channels");
    assert!(sample_rate > 0);
    assert!(channels > 0);
    std::thread::sleep(std::time::Duration::from_millis(100));
    let underruns = playback.underrun_count();
    eprintln!("playback silence periods: underrun_count={underruns}");
    assert!(
        underruns > 0,
        "expected the empty-queue worker to report at least one silence-filled period"
    );
    playback.close().expect("close");
}
