#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use super::*;
use crate::Select;
use crate::audio::{AudioCaptureConfig, AudioPlayback, AudioPlaybackConfig};
use mediaway_common::Rational;

#[test]
fn open_wasapi_mic_poll_or_skip() {
    let _guard = crate::windows_audio::HARDWARE_TEST_LOCK
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
    let _guard = crate::windows_audio::HARDWARE_TEST_LOCK
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

/// Endpoint loopback keeps its sample count on the wall clock through silence: captures a
/// render endpoint for 1.5 s while this test plays nothing, then plays a 0.5 s 440 Hz sine at
/// **-74 dBFS** (amplitude 0.0002 — far too quiet to hear) to the same endpoint, and checks
/// that (a) frame `pts` are contiguous, (b) the samples delivered match the wall time since
/// `open`, and (c) the tone's first sample sits at the wall time it started playing — not
/// 1.5 s early, which is where it landed while silent packets were dropped and an idle
/// endpoint's missing packets went uncounted (ADR-0002 § Correction (2026-10-13)).
///
/// The endpoint: `MEDIAWAY_LOOPBACK_ENDPOINT` (a name substring, e.g. `Virtual Audio Cable`
/// for an endpoint nothing else plays to, which is the idle case), else the default one. If
/// something else is audible on it before the tone, (c) is reported and skipped.
///
/// ```text
/// MEDIAWAY_LOOPBACK_ENDPOINT="Virtual Audio Cable" cargo nextest run -p mediaway-device --lib \
///   --run-ignored only -E 'test(loopback_sample_clock_follows_wall_time_through_silence)' --no-capture
/// ```
#[test]
#[ignore = "plays a -74 dBFS tone on a real render endpoint; run explicitly"]
#[allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    reason = "one scenario"
)]
fn loopback_sample_clock_follows_wall_time_through_silence() {
    use crate::windows_audio::{WasapiCaptureConfig, WasapiSource};
    use mediaway_common::{AudioFrame, Bytes, SampleFormat};
    use std::time::{Duration, Instant};

    let _guard = crate::windows_audio::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let select =
        std::env::var("MEDIAWAY_LOOPBACK_ENDPOINT").map_or(Select::Default, Select::NameContains);
    let mut capture = match WindowsWasapiCapture::open(&WasapiCaptureConfig {
        source: WasapiSource::Loopback {
            select: select.clone(),
        },
        time_base: Rational::new(1, 48_000),
        sample_format: SampleFormat::F32,
    }) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skip: loopback open ({e:?})");
            return;
        }
    };
    let opened = Instant::now();
    let rate = capture.stream_info().sample_rate().expect("rate");
    let channels = usize::from(capture.stream_info().channels().expect("channels"));

    let mut first_channel: Vec<f32> = Vec::new();
    let mut next_pts = 0i64;
    let mut drain = |capture: &mut WindowsWasapiCapture, until: Instant| {
        while Instant::now() < until {
            while let Some(frame) = capture.poll_frame().expect("poll") {
                assert_eq!(frame.pts, next_pts, "frames are contiguous");
                next_pts += i64::try_from(frame.duration).expect("duration");
                for sample in frame.data.chunks_exact(4 * channels) {
                    let bytes = [sample[0], sample[1], sample[2], sample[3]];
                    first_channel.push(f32::from_le_bytes(bytes));
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    };

    drain(&mut capture, opened + Duration::from_millis(1_500));

    let mut out = match WindowsWasapiPlayback::open(&AudioPlaybackConfig {
        select,
        sample_format: SampleFormat::F32,
    }) {
        Ok(out) => out,
        Err(e) => {
            eprintln!("skip: playback open ({e:?})");
            return;
        }
    };
    let out_rate = out.stream_info().sample_rate().expect("rate");
    let out_channels = out.stream_info().channels().expect("channels");
    let per = out_rate / 100;
    let tone_at = opened.elapsed();
    for period in 0..50u32 {
        let mut data = Vec::with_capacity(per as usize * usize::from(out_channels) * 4);
        for i in 0..per {
            let t = f64::from(period * per + i) / f64::from(out_rate);
            #[allow(clippy::cast_possible_truncation, reason = "a test tone")]
            let v = (0.0002 * (t * 440.0 * std::f64::consts::TAU).sin()) as f32;
            for _ in 0..out_channels {
                data.extend_from_slice(&v.to_le_bytes());
            }
        }
        let mut frame = AudioFrame {
            pts: 0,
            duration: u64::from(per),
            sample_rate: out_rate,
            channels: out_channels,
            format: SampleFormat::F32,
            data: Bytes::from(data),
        };
        loop {
            match out.write_frame(frame) {
                Err(crate::audio::PlaybackError::QueueFull(back)) => {
                    frame = back;
                    std::thread::sleep(Duration::from_millis(5));
                }
                written => {
                    written.expect("playback write");
                    break;
                }
            }
        }
    }
    drain(&mut capture, opened + Duration::from_millis(2_500));
    out.close().expect("close playback");
    drain(&mut capture, opened + Duration::from_millis(3_000));
    let elapsed = opened.elapsed().as_secs_f64();
    capture.close().expect("close capture");

    let delivered = first_channel.len() as f64 / f64::from(rate);
    let onset = first_channel.iter().position(|s| s.abs() > 1.0e-4);
    let loud_before = first_channel
        .iter()
        .take(usize::try_from(tone_at.as_micros() * u128::from(rate) / 1_000_000).expect("fits"))
        .any(|s| s.abs() > 1.0e-5);
    eprintln!(
        "delivered {delivered:.3} s of {elapsed:.3} s wall; tone written at {:.3} s; onset at {:?} s; other sound before the tone: {loud_before}",
        tone_at.as_secs_f64(),
        onset.map(|i| i as f64 / f64::from(rate)),
    );
    // Idle fill trails the wall clock by up to 120 ms (`IDLE_LAG_HNS` + one piece).
    assert!(
        (elapsed - delivered).abs() < 0.2,
        "delivered {delivered:.3} s of sound in {elapsed:.3} s"
    );
    if loud_before {
        eprintln!("skip onset check: the endpoint was not silent before the tone");
        return;
    }
    let onset = onset.expect("the tone was captured") as f64 / f64::from(rate);
    // The tone reaches the loopback a render buffer's latency after it is written.
    let late = onset - tone_at.as_secs_f64();
    assert!(
        (-0.02..0.15).contains(&late),
        "tone written at {:.3} s was captured at {onset:.3} s",
        tone_at.as_secs_f64()
    );
}
