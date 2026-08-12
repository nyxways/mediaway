//! Android microphone capture via AAudio (`ndk::audio`, blocking `read()` path).
//!
//! See [ADR-0002](adr/android/0002-aaudio-microphone-capture.md). AAudio's `data_callback`
//! model is deliberately **not** used here — its own doc comments forbid taking a mutex inside
//! it, which this crate's shared `Arc<Mutex<VecDeque<AudioFrame>>>` queue shape (mirroring
//! `linux::mic`/`mediaway-device-windows` `wasapi.rs`) would violate. The blocking `read()` path
//! trades some latency for reusing that already-hardened queue shape instead.
//!
//! **Zero compile verification** — this dev environment has no Android NDK toolchain; see the
//! crate's `android` CI job.

#![allow(unsafe_code)]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::audio::{AudioCapture, AudioCaptureConfig};
use crate::{CaptureError, Select};
use mediaway_common::{AudioFrame, Bytes, CodecKind, Rational, SampleFormat, StreamInfo};
use ndk::audio::{
    AudioDirection, AudioFormat, AudioPerformanceMode, AudioSharingMode, AudioStream,
    AudioStreamBuilder,
};

/// Bounded, drop-oldest PCM queue depth — mirrors `linux::mic`'s `PCM_QUEUE_CAP`.
const PCM_QUEUE_CAP: usize = 64;

/// Frames requested per blocking `read()` call — a fixed, small chunk (not tied to the
/// negotiated sample rate) so the read timeout below stays a short, predictable wait
/// regardless of the device's actual rate.
const READ_CHUNK_FRAMES: i32 = 480;

/// `AudioStream::read`'s timeout — bounds how long the worker can be blocked before it rechecks
/// the stop flag, same "wait up to one read interval" contract V4L2's `STREAM_POLL_TIMEOUT`
/// documents for its own stop-flag responsiveness.
const READ_TIMEOUT_NS: i64 = 20_000_000;

struct SharedQueue {
    frames: Mutex<VecDeque<AudioFrame>>,
}

struct MicSession {
    stream_info: StreamInfo,
    queue: Arc<SharedQueue>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

/// Android microphone capture via an AAudio input stream (`Shared` sharing mode,
/// `LowLatency` performance mode, system default input device only this slice).
pub struct AndroidMicrophoneCapture {
    inner: Option<MicSession>,
}

impl AndroidMicrophoneCapture {
    /// Open AAudio microphone capture for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Unsupported`] for a non-[`Select::Default`] selection (AAudio has
    /// no NDK input-device enumeration API — only the system default input, `device_id(0)`, is
    /// reachable without a JNI round trip into `android.media.AudioManager`) or a non-`F32`
    /// `sample_format`. Returns [`CaptureError::InvalidInput`] for a zero-denominator time base.
    /// Returns [`CaptureError::Backend`] when the AAudio stream fails to build or open.
    pub fn open(config: &AudioCaptureConfig) -> Result<Self, CaptureError> {
        if config.select != Select::Default {
            return Err(CaptureError::Unsupported);
        }
        if config.sample_format != SampleFormat::F32 {
            return Err(CaptureError::Unsupported);
        }
        if config.time_base.den == 0 {
            return Err(CaptureError::InvalidInput);
        }

        let queue = Arc::new(SharedQueue {
            frames: Mutex::new(VecDeque::new()),
        });
        // clone: worker thread needs its own strong ref to push frames
        let queue_worker = Arc::clone(&queue);
        let stop = Arc::new(AtomicBool::new(false));
        // clone: Arc share with mic worker thread
        let stop_worker = Arc::clone(&stop);
        let time_base = config.time_base;

        let (tx_info, rx_info) = std::sync::mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("mediaway-aaudio-mic".into())
            .spawn(move || {
                run_mic_worker(time_base, &queue_worker, &stop_worker, &tx_info);
            })
            .map_err(|_| CaptureError::Backend)?;

        let stream_info = rx_info.recv().map_err(|_| CaptureError::Backend)??;

        Ok(Self {
            inner: Some(MicSession {
                stream_info,
                queue,
                stop,
                worker: Some(worker),
            }),
        })
    }
}

impl AudioCapture for AndroidMicrophoneCapture {
    fn stream_info(&self) -> &StreamInfo {
        #[allow(
            clippy::option_if_let_else,
            reason = "map_or_else forces 'static vs 'self lifetime clash"
        )]
        if let Some(s) = self.inner.as_ref() {
            &s.stream_info
        } else {
            closed_audio_info()
        }
    }

    fn poll_frame(&mut self) -> Result<Option<AudioFrame>, CaptureError> {
        let Some(session) = self.inner.as_ref() else {
            return Err(CaptureError::Closed);
        };
        let mut q = session
            .queue
            .frames
            .lock()
            .map_err(|_| CaptureError::Backend)?;
        Ok(q.pop_front())
    }

    /// Signals the worker's stop flag and joins it. `AudioStream::read` blocks up to
    /// [`READ_TIMEOUT_NS`] before the worker notices `stop` — same "wait up to one read
    /// interval" contract `linux::camera`/`linux::mic` document for their own stop paths.
    fn close(&mut self) -> Result<(), CaptureError> {
        let Some(mut session) = self.inner.take() else {
            return Ok(());
        };
        session.stop.store(true, Ordering::SeqCst);
        if let Some(h) = session.worker.take() {
            let _ = h.join();
        }
        Ok(())
    }
}

impl Drop for AndroidMicrophoneCapture {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn closed_audio_info() -> &'static StreamInfo {
    use std::sync::OnceLock;
    static INFO: OnceLock<StreamInfo> = OnceLock::new();
    INFO.get_or_init(|| StreamInfo::Audio {
        id: 0,
        codec: CodecKind::RawAudio,
        time_base: Rational::new(1, 48_000),
        sample_rate: 0,
        channels: 0,
        extra_data: Bytes::new(),
    })
}

fn run_mic_worker(
    time_base: Rational,
    queue: &SharedQueue,
    stop: &AtomicBool,
    tx_info: &SyncSender<Result<StreamInfo, CaptureError>>,
) {
    let stream = match open_stream() {
        Ok(s) => s,
        Err(e) => {
            let _ = tx_info.send(Err(e));
            return;
        }
    };

    let channels = stream.channel_count();
    let sample_rate = stream.sample_rate();
    if channels <= 0 || sample_rate <= 0 {
        let _ = tx_info.send(Err(CaptureError::Backend));
        return;
    }
    let info = StreamInfo::Audio {
        id: 0,
        codec: CodecKind::RawAudio,
        time_base,
        sample_rate: sample_rate.unsigned_abs(),
        channels: u16::try_from(channels).unwrap_or(0),
        extra_data: Bytes::new(),
    };

    if stream.request_start().is_err() {
        let _ = tx_info.send(Err(CaptureError::Backend));
        return;
    }
    let _ = tx_info.send(Ok(info));

    let channels = channels as usize;
    let mut buffer = vec![0f32; READ_CHUNK_FRAMES as usize * channels];
    let mut pts: i64 = 0;
    while !stop.load(Ordering::Relaxed) {
        // SAFETY: `buffer` is a valid, initialized `&mut [f32]` with capacity for
        // `READ_CHUNK_FRAMES * channels` samples — at least `READ_CHUNK_FRAMES` frames at the
        // stream's own negotiated `channels`, matching this function's `# Safety` contract.
        let read = unsafe {
            stream.read(
                buffer.as_mut_ptr().cast(),
                READ_CHUNK_FRAMES,
                READ_TIMEOUT_NS,
            )
        };
        match read {
            Ok(0) => {}
            Ok(frames) => {
                let frames = frames as usize;
                let sample_count = frames.saturating_mul(channels);
                if let Some(chunk) = buffer.get(..sample_count) {
                    push_frame(queue, sample_rate.unsigned_abs(), channels, pts, chunk);
                    pts = pts.saturating_add(i64::try_from(frames).unwrap_or(0));
                }
            }
            Err(_) => break,
        }
    }
    let _ = stream.request_stop();
    // `stream`'s `Drop` issues `AAudioStream_close`.
}

fn open_stream() -> Result<AudioStream, CaptureError> {
    AudioStreamBuilder::new()
        .map_err(|_| CaptureError::Backend)?
        .direction(AudioDirection::Input)
        .format(AudioFormat::PCM_Float)
        .sharing_mode(AudioSharingMode::Shared)
        .performance_mode(AudioPerformanceMode::LowLatency)
        .device_id(0)
        .open_stream()
        .map_err(|_| CaptureError::Backend)
}

fn push_frame(queue: &SharedQueue, sample_rate: u32, channels: usize, pts: i64, samples: &[f32]) {
    let frame = AudioFrame {
        pts,
        duration: u64::try_from(samples.len() / channels.max(1)).unwrap_or(0),
        sample_rate,
        channels: u16::try_from(channels).unwrap_or(0),
        format: SampleFormat::F32,
        // clone: `buffer` is reused by the worker loop on its next `read()`, so the
        // caller-owned `AudioFrame` must outlive it — same rationale as `linux::mic`'s
        // `push_frame`.
        data: Bytes::copy_from_slice(bytemuck_f32_to_bytes(samples)),
    };
    if let Ok(mut q) = queue.frames.lock() {
        if q.len() >= PCM_QUEUE_CAP {
            let _ = q.pop_front();
        }
        q.push_back(frame);
    }
}

/// Reinterpret an `f32` PCM chunk as its little-endian byte representation — AAudio's
/// `PCM_Float` samples are native-endian `f32`, which is little-endian on every Android ABI
/// this crate targets (`arm64-v8a`/`armeabi-v7a`/`x86`/`x86_64`).
fn bytemuck_f32_to_bytes(samples: &[f32]) -> &[u8] {
    // SAFETY: `f32` has no padding/invalid bit patterns and `[f32]`'s alignment is a multiple
    // of `[u8]`'s — reinterpreting the slice's byte length is always in-bounds.
    unsafe {
        std::slice::from_raw_parts(
            samples.as_ptr().cast::<u8>(),
            std::mem::size_of_val(samples),
        )
    }
}

#[cfg(test)]
#[path = "mic_tests.rs"]
mod tests;
