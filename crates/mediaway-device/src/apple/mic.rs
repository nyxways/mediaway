//! Apple microphone capture via `AVAudioEngine`'s `inputNode` tap — **not** `AVCaptureSession` +
//! `AVCaptureAudioDataOutput`. See [ADR-0002](adr/apple/0002-avaudioengine-microphone-capture.md).
//!
//! `AVAudioEngine` is purpose-built pure-audio-capture: no `AVCaptureSession`/device-input/
//! delegate-class machinery, and no `CMBlockBuffer` extraction (`AVAudioPCMBuffer` arrives
//! ready-shaped, unlike a `CMSampleBuffer`). The one real cost this domain alone pays among this
//! crate's mic backends: `AVAudioPCMBuffer::floatChannelData` is **planar** (one pointer per
//! channel), but [`mediaway_common::AudioFrame::data`] is documented interleaved — every
//! callback invocation interleaves the planar channels into one owned buffer (`interleave_pcm_f32`,
//! named per `docs/spec/caveats-and-clarity.md`'s honest-cost-naming rule).
//!
//! **Zero compile verification** — this dev environment cannot cross-compile Apple code at all
//! outside macOS/Xcode; see the crate's `apple-macos`/`apple-ios` CI jobs.

#![allow(unsafe_code)]

use std::collections::VecDeque;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use crate::audio::{AudioCapture, AudioCaptureConfig};
use crate::{CaptureError, Select};
use block2::RcBlock;
use mediaway_common::{AudioFrame, Bytes, CodecKind, Rational, SampleFormat, StreamInfo};
use objc2::rc::Retained;
use objc2_avf_audio::{AVAudioEngine, AVAudioPCMBuffer, AVAudioTime};

/// Requested tap buffer size in frames — within `installTapOnBus:bufferSize:format:...`'s
/// documented supported range ([100, 400] ms) for common sample rates (e.g. ~93 ms at 44.1 kHz).
const TAP_BUFFER_FRAMES: u32 = 4096;

struct SharedQueue {
    frames: Mutex<VecDeque<AudioFrame>>,
}

/// Bounded, drop-oldest PCM queue depth — mirrors `android::mic`'s `PCM_QUEUE_CAP`.
const PCM_QUEUE_CAP: usize = 64;

struct MicSession {
    stream_info: StreamInfo,
    queue: Arc<SharedQueue>,
    engine: Retained<AVAudioEngine>,
    // Kept alive for the whole session — `installTapOnBus...` does not take ownership of the
    // block, but this crate's own convention (mirrors AAudio's boxed callbacks) is to hold every
    // callback/delegate object alive explicitly rather than relying solely on the OS API's own
    // internal retain.
    //
    // `RcBlock<F>` takes exactly one generic parameter — the `dyn Fn(...)` block signature itself
    // (see `block2::RcBlock`'s own doc example, `RcBlock<dyn Fn(&'a i32) + 'b>`), not a separate
    // lifetime/fn-pointer/marker-trait triple.
    _tap_block: RcBlock<dyn Fn(NonNull<AVAudioPCMBuffer>, NonNull<AVAudioTime>)>,
}

/// Apple microphone capture via an `AVAudioEngine` input tap (system default input only this
/// slice — `AVAudioEngine.inputNode` has no per-device selection API without a separate
/// `AVAudioSession` dependency this backend does not add).
pub struct AppleMicrophoneCapture {
    inner: Option<MicSession>,
}

impl AppleMicrophoneCapture {
    /// Open `AVAudioEngine` microphone capture for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Unsupported`] for a non-[`Select::Default`] selection (no
    /// per-device selection this slice) or a non-`F32` `sample_format`. Returns
    /// [`CaptureError::InvalidInput`] for a zero-denominator time base. Returns
    /// [`CaptureError::Backend`] when the input node reports an unusable (zero) format or the
    /// engine fails to start.
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

        // SAFETY: `AVAudioEngine::new()` is a plain, always-safe-to-call Foundation
        // constructor (no preconditions beyond the Objective-C runtime being initialized,
        // guaranteed on any Apple process).
        let engine = unsafe { AVAudioEngine::new() };
        // SAFETY: `engine` is a valid, just-created `AVAudioEngine`.
        let input = unsafe { engine.inputNode() };
        // SAFETY: `input` is a valid node belonging to `engine`; bus `0` is the input node's
        // only bus.
        let format = unsafe { input.outputFormatForBus(0) };
        // SAFETY: `format` is a valid, just-obtained `AVAudioFormat`.
        let (sample_rate, channels) = unsafe { (format.sampleRate(), format.channelCount()) };
        if !(sample_rate > 0.0) || channels == 0 {
            return Err(CaptureError::Backend);
        }
        let channels_usize = channels as usize;
        // `sample_rate > 0.0` is checked just above; real audio sample rates (e.g. 44100/48000)
        // are always small positive integers, exact in `u32`.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "sample_rate > 0.0 checked above; real sample rates are small positive integers"
        )]
        let sample_rate_u32 = sample_rate as u32;

        let queue = Arc::new(SharedQueue {
            frames: Mutex::new(VecDeque::new()),
        });
        // clone: tap block closure needs its own strong ref to push frames
        let queue_tap = Arc::clone(&queue);
        let next_pts = Arc::new(AtomicI64::new(0));

        let tap_block: RcBlock<dyn Fn(NonNull<AVAudioPCMBuffer>, NonNull<AVAudioTime>)> =
            RcBlock::new(
                move |buf: NonNull<AVAudioPCMBuffer>, _when: NonNull<AVAudioTime>| {
                    // SAFETY: `buf` is a valid, non-null `AVAudioPCMBuffer` for the duration of this
                    // callback invocation — Apple's documented contract for `AVAudioNodeTapBlock`.
                    let buf = unsafe { buf.as_ref() };
                    // SAFETY: same buffer, same callback-duration validity.
                    let frame_length = unsafe { buf.frameLength() } as usize;
                    if frame_length == 0 {
                        return;
                    }
                    // SAFETY: same buffer; `floatChannelData` returns an array of `channels_usize`
                    // per-channel pointers, each valid for `frame_length` `f32` samples, for the
                    // duration of this callback — Apple's documented `AVAudioPCMBuffer` contract.
                    let channel_ptrs = unsafe { buf.floatChannelData() };
                    if channel_ptrs.is_null() {
                        return;
                    }
                    // SAFETY: `channel_ptrs` points to `channels_usize` valid, non-null `f32`
                    // pointers (checked above), each valid for `frame_length` elements — same
                    // `AVAudioPCMBuffer` contract as the `floatChannelData` call above.
                    let interleaved =
                        unsafe { interleave_pcm_f32(channel_ptrs, channels_usize, frame_length) };
                    let pts = next_pts
                        .fetch_add(i64::try_from(frame_length).unwrap_or(0), Ordering::Relaxed);
                    push_frame(
                        &queue_tap,
                        sample_rate_u32,
                        channels_usize,
                        pts,
                        &interleaved,
                    );
                },
            );

        // SAFETY: `input` is a valid input node with no tap currently installed (just created);
        // `format: None` reuses the node's own current format per the doc comment's guidance for
        // tapping an input bus; the raw block pointer is kept alive by this session's own
        // `_tap_block` field for the whole session, matching `AVAudioNodeTapBlock`'s real
        // parameter type (`*mut block2::DynBlock<...>`, not a `&RcBlock<...>` reference).
        unsafe {
            input.installTapOnBus_bufferSize_format_block(
                0,
                TAP_BUFFER_FRAMES,
                None,
                RcBlock::as_ptr(&tap_block),
            );
        }

        // SAFETY: `engine` has a tap installed and is ready to start.
        unsafe { engine.startAndReturnError() }.map_err(|_| CaptureError::Backend)?;

        let info = StreamInfo::Audio {
            id: 0,
            codec: CodecKind::RawAudio,
            time_base: config.time_base,
            sample_rate: sample_rate_u32,
            channels: u16::try_from(channels).unwrap_or(0),
            extra_data: Bytes::new(),
        };

        Ok(Self {
            inner: Some(MicSession {
                stream_info: info,
                queue,
                engine,
                _tap_block: tap_block,
            }),
        })
    }
}

impl AudioCapture for AppleMicrophoneCapture {
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

    fn close(&mut self) -> Result<(), CaptureError> {
        let Some(session) = self.inner.take() else {
            return Ok(());
        };
        // SAFETY: `session.engine` is a valid, live `AVAudioEngine`.
        unsafe { session.engine.stop() };
        // `session`'s `Drop` (via `Retained`/`RcBlock`) releases the engine and tap block.
        Ok(())
    }
}

impl Drop for AppleMicrophoneCapture {
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

/// Interleave `channels` per-channel planar `f32` buffers (each `frame_length` samples, pointed
/// to by `channel_ptrs[0..channels]`) into one owned `Vec<f32>` — the real, named cost this
/// domain alone pays among this crate's mic backends (see module docs). Pure pointer-to-slice
/// copy, no FFI calls, so this is unit-testable with synthetic pointers built from local arrays.
///
/// # Safety
///
/// `channel_ptrs` must point to at least `channels` valid, non-null `f32` pointers, each valid
/// for reads of `frame_length` elements.
unsafe fn interleave_pcm_f32(
    channel_ptrs: *mut NonNull<f32>,
    channels: usize,
    frame_length: usize,
) -> Vec<f32> {
    let mut out = vec![0f32; frame_length * channels];
    for ch in 0..channels {
        // SAFETY: caller's contract guarantees `channel_ptrs.add(ch)` is a valid, initialized
        // `NonNull<f32>` pointer readable for `frame_length` elements.
        let channel_ptr = unsafe { *channel_ptrs.add(ch) };
        // SAFETY: same contract — `channel_ptr` is valid for `frame_length` reads.
        let channel_slice =
            unsafe { std::slice::from_raw_parts(channel_ptr.as_ptr(), frame_length) };
        for (frame, &sample) in channel_slice.iter().enumerate() {
            out[frame * channels + ch] = sample;
        }
    }
    out
}

fn push_frame(queue: &SharedQueue, sample_rate: u32, channels: usize, pts: i64, samples: &[f32]) {
    let frame = AudioFrame {
        pts,
        duration: u64::try_from(samples.len() / channels.max(1)).unwrap_or(0),
        sample_rate,
        channels: u16::try_from(channels).unwrap_or(0),
        format: SampleFormat::F32,
        // clone: `samples` is a freshly interleaved `Vec` local to this callback invocation, but
        // `Bytes` still needs its own byte view — copy avoided where possible is not applicable
        // here since `samples` is already this callback's own owned allocation; this reinterprets
        // it as bytes without an extra copy (see `bytes_from_f32_vec`).
        data: bytes_from_f32_vec(samples),
    };
    if let Ok(mut q) = queue.frames.lock() {
        if q.len() >= PCM_QUEUE_CAP {
            let _ = q.pop_front();
        }
        q.push_back(frame);
    }
}

/// Reinterpret an owned `f32` PCM vector as its little-endian byte representation — no
/// vendor copy beyond the interleave step already performed in `interleave_pcm_f32`.
fn bytes_from_f32_vec(samples: &[f32]) -> Bytes {
    // SAFETY: `f32` has no padding/invalid bit patterns and `[f32]`'s alignment is a multiple of
    // `[u8]`'s — reinterpreting the slice's byte length is always in-bounds.
    let bytes = unsafe {
        std::slice::from_raw_parts(
            samples.as_ptr().cast::<u8>(),
            std::mem::size_of_val(samples),
        )
    };
    Bytes::copy_from_slice(bytes)
}

#[cfg(test)]
#[path = "mic_tests.rs"]
mod tests;
