//! [`AudioProcessor`] — echo cancellation (AEC3) + noise suppression (NS) +
//! gain control (AGC2), via `sonora::AudioProcessing`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mediaway_common::{AudioFrame, SampleFormat};
use sonora::{AudioProcessing, StreamConfig};

use crate::ApmConfig;
use crate::error::ApmError;
use crate::pcm::{bytes_to_f32, f32_to_bytes};

/// Sample format / rate / channel layout for one side of an [`AudioProcessor`].
///
/// `sample_format` must be [`SampleFormat::F32`] — `sonora` processes `f32`
/// PCM only (matches `AudioCaptureConfig::microphone()`'s existing default
/// in `mediaway-device`). `channels` must be non-zero — a zero value never
/// accumulates a complete 10ms block, so an [`AudioProcessor`] configured
/// with it never produces output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioStreamFormat {
    /// Sample rate (Hz). `sonora` accepts `8_000..=384_000`.
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u16,
    /// PCM sample format — must be [`SampleFormat::F32`].
    pub sample_format: SampleFormat,
}

/// Echo cancellation (AEC3) + noise suppression (NS) + gain control (AGC2),
/// via `sonora::AudioProcessing`.
///
/// The engine consumes two input streams: **render** (far-end / about-to-be
/// -played audio, fed via [`push_render_frame`](Self::push_render_frame),
/// used only as an echo reference) and **capture** (near-end / microphone
/// audio, fed via [`push_capture_frame`](Self::push_capture_frame) and
/// pulled back out via [`poll_processed_frame`](Self::poll_processed_frame)
/// once enhanced). `sonora` processes fixed 10ms blocks regardless of the
/// push chunk size — both streams are re-blocked internally.
///
/// # Not Zero-Copy
/// `sonora`'s API takes separate deinterleaved `f32` src/dst slices while
/// [`AudioFrame`] carries interleaved bytes; converting between the two, and
/// re-blocking arbitrary push sizes into fixed 10ms blocks, is a real
/// payload copy on every call. Never Zero-Copy here — see
/// `docs/ai/wiki/zero-copy/marks.md`.
#[derive(Debug)]
pub struct AudioProcessor {
    /// `None` after a caught backend panic — see [`is_disabled`](Self::is_disabled).
    inner: Option<AudioProcessing>,
    capture_format: AudioStreamFormat,
    render_format: AudioStreamFormat,
    /// Samples-per-channel in one 10ms block (`sample_rate / 100`).
    capture_block_frames: usize,
    render_block_frames: usize,
    capture_channels: usize,
    render_channels: usize,
    /// Interleaved raw samples awaiting a full block.
    capture_accum: Vec<f32>,
    render_accum: Vec<f32>,
    /// Reused deinterleaved scratch buffers, indexed by channel.
    capture_in: Vec<Vec<f32>>,
    capture_out: Vec<Vec<f32>>,
    render_in: Vec<Vec<f32>>,
    render_out: Vec<Vec<f32>>,
    /// Running per-channel sample-count `pts` for the next produced block —
    /// `None` until the first sample is pushed. See
    /// [`poll_processed_frame`](Self::poll_processed_frame).
    next_capture_pts: Option<i64>,
}

/// Splits `channels * frames` interleaved samples into per-channel scratch
/// buffers (each already sized to `frames`).
fn deinterleave(interleaved: &[f32], channels: usize, out: &mut [Vec<f32>]) {
    if channels == 0 {
        return;
    }
    let frames = interleaved.len() / channels;
    for (ch, out_ch) in out.iter_mut().enumerate().take(channels) {
        for (f, sample) in out_ch.iter_mut().enumerate().take(frames) {
            *sample = interleaved[f * channels + ch];
        }
    }
}

/// Inverse of [`deinterleave`] — packs `channels` per-channel buffers of
/// `frames` samples each back into one interleaved `Vec<f32>`.
fn interleave(channel_data: &[Vec<f32>], channels: usize, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0_f32; channels * frames];
    for (ch, buf) in channel_data.iter().enumerate() {
        for (f, &sample) in buf.iter().enumerate().take(frames) {
            out[f * channels + ch] = sample;
        }
    }
    out
}

/// Validates `frame` against `expected` — format must be `F32` and the
/// sample rate/channel count must match exactly.
fn validate_frame(frame: &AudioFrame, expected: AudioStreamFormat) -> Result<(), ApmError> {
    if frame.format != SampleFormat::F32 {
        return Err(ApmError::UnsupportedSampleFormat(frame.format));
    }
    if frame.sample_rate != expected.sample_rate || frame.channels != expected.channels {
        return Err(ApmError::StreamFormatMismatch {
            expected_sample_rate: expected.sample_rate,
            expected_channels: expected.channels,
            actual_sample_rate: frame.sample_rate,
            actual_channels: frame.channels,
        });
    }
    Ok(())
}

impl AudioProcessor {
    /// Opens a new processor for the given capture/render stream shapes.
    ///
    /// # Errors
    /// [`ApmError::UnsupportedSampleFormat`] if either format's
    /// `sample_format != SampleFormat::F32`. [`ApmError::BackendPanicked`]
    /// if `sonora`'s builder panicked while constructing the instance (see
    /// this crate's panic-safety posture).
    pub fn open(
        config: ApmConfig,
        capture_format: AudioStreamFormat,
        render_format: AudioStreamFormat,
    ) -> Result<Self, ApmError> {
        if capture_format.sample_format != SampleFormat::F32 {
            return Err(ApmError::UnsupportedSampleFormat(
                capture_format.sample_format,
            ));
        }
        if render_format.sample_format != SampleFormat::F32 {
            return Err(ApmError::UnsupportedSampleFormat(
                render_format.sample_format,
            ));
        }

        let capture_stream = StreamConfig::new(capture_format.sample_rate, capture_format.channels);
        let render_stream = StreamConfig::new(render_format.sample_rate, render_format.channels);

        let build_result = catch_unwind(AssertUnwindSafe(|| {
            AudioProcessing::builder()
                .config(config)
                .capture_config(capture_stream)
                .render_config(render_stream)
                .build()
        }));
        let inner = match build_result {
            Ok(apm) => Some(apm),
            Err(_) => return Err(ApmError::BackendPanicked),
        };

        let capture_channels = usize::from(capture_format.channels);
        let render_channels = usize::from(render_format.channels);
        let capture_block_frames = capture_stream.num_frames();
        let render_block_frames = render_stream.num_frames();

        Ok(Self {
            inner,
            capture_format,
            render_format,
            capture_block_frames,
            render_block_frames,
            capture_channels,
            render_channels,
            capture_accum: Vec::new(),
            render_accum: Vec::new(),
            capture_in: vec![vec![0.0; capture_block_frames]; capture_channels],
            capture_out: vec![vec![0.0; capture_block_frames]; capture_channels],
            render_in: vec![vec![0.0; render_block_frames]; render_channels],
            render_out: vec![vec![0.0; render_block_frames]; render_channels],
            next_capture_pts: None,
        })
    }

    /// Feed a render-reference (far-end / about-to-be-played) frame. No
    /// output — this only updates the internal echo reference `sonora`'s
    /// capture path later consumes. No-op once [`is_disabled`](Self::is_disabled).
    ///
    /// # Errors
    /// [`ApmError::UnsupportedSampleFormat`] / [`ApmError::StreamFormatMismatch`]
    /// if `frame`'s format doesn't match the render format this instance was
    /// opened with. [`ApmError::BackendPanicked`] if `sonora` panicked while
    /// processing this block (this instance is now disabled).
    /// [`ApmError::Backend`] if `sonora` returned a typed (non-panic) error.
    pub fn push_render_frame(&mut self, frame: &AudioFrame) -> Result<(), ApmError> {
        if self.inner.is_none() {
            return Ok(());
        }
        validate_frame(frame, self.render_format)?;

        self.render_accum.extend(bytes_to_f32(&frame.data));

        let block_len = self.render_block_frames * self.render_channels;
        while block_len > 0 && self.render_accum.len() >= block_len {
            deinterleave(
                &self.render_accum[..block_len],
                self.render_channels,
                &mut self.render_in,
            );
            self.render_accum.drain(..block_len);

            let Some(inner) = self.inner.as_mut() else {
                return Ok(());
            };
            let src: Vec<&[f32]> = self.render_in.iter().map(Vec::as_slice).collect();
            let mut dst: Vec<&mut [f32]> =
                self.render_out.iter_mut().map(Vec::as_mut_slice).collect();
            let result = catch_unwind(AssertUnwindSafe(|| {
                inner.process_render_f32(&src, &mut dst)
            }));
            match result {
                Ok(Ok(())) => {}
                Ok(Err(err)) => return Err(ApmError::Backend(err)),
                Err(_) => {
                    self.inner = None;
                    return Err(ApmError::BackendPanicked);
                }
            }
        }
        Ok(())
    }

    /// Feed a near-end (mic) capture frame. Buffered internally into 10ms
    /// blocks — does not synchronously return output (see
    /// [`poll_processed_frame`](Self::poll_processed_frame)). Buffering
    /// continues even after [`is_disabled`](Self::is_disabled) — the
    /// underlying raw PCM is still valid and is what
    /// `poll_processed_frame` passes through.
    ///
    /// # Errors
    /// [`ApmError::UnsupportedSampleFormat`] / [`ApmError::StreamFormatMismatch`]
    /// if `frame`'s format doesn't match the capture format this instance
    /// was opened with.
    pub fn push_capture_frame(&mut self, frame: &AudioFrame) -> Result<(), ApmError> {
        validate_frame(frame, self.capture_format)?;

        if self.next_capture_pts.is_none() {
            self.next_capture_pts = Some(frame.pts);
        }
        self.capture_accum.extend(bytes_to_f32(&frame.data));
        Ok(())
    }

    /// Pull the next processed 10ms capture block, if a full block is ready.
    /// After a caught backend panic — i.e. on every call *after* the one
    /// that returned [`ApmError::BackendPanicked`] — returns the
    /// **unmodified** input block (documented passthrough; see this crate's
    /// panic-safety posture), never silently disguised as a normally
    /// processed one from the caller's point of view: check
    /// [`is_disabled`](Self::is_disabled).
    ///
    /// `pts` on the returned frame is derived from the first pushed frame's
    /// `pts`, advanced by one block's worth of samples (`sample_rate / 100`)
    /// per produced block — an approximation (per-push `pts` values are not
    /// individually tracked once buffered), not a precise per-sample replay.
    ///
    /// # Errors
    /// [`ApmError::BackendPanicked`] on the call during which `sonora`
    /// panicked (subsequent calls pass through instead — see above).
    /// [`ApmError::Backend`] if `sonora` returned a typed (non-panic) error.
    pub fn poll_processed_frame(&mut self) -> Result<Option<AudioFrame>, ApmError> {
        let block_len = self.capture_block_frames * self.capture_channels;
        if block_len == 0 || self.capture_accum.len() < block_len {
            return Ok(None);
        }

        let base_pts = self.next_capture_pts.unwrap_or(0);
        let block_frames_i64 = i64::try_from(self.capture_block_frames).unwrap_or(i64::MAX);

        let Some(inner) = self.inner.as_mut() else {
            let block: Vec<f32> = self.capture_accum.drain(..block_len).collect();
            self.next_capture_pts = Some(base_pts.saturating_add(block_frames_i64));
            return Ok(Some(self.raw_capture_frame(base_pts, &block)));
        };

        deinterleave(
            &self.capture_accum[..block_len],
            self.capture_channels,
            &mut self.capture_in,
        );
        let src: Vec<&[f32]> = self.capture_in.iter().map(Vec::as_slice).collect();
        let mut dst: Vec<&mut [f32]> = self.capture_out.iter_mut().map(Vec::as_mut_slice).collect();
        let result = catch_unwind(AssertUnwindSafe(|| {
            inner.process_capture_f32(&src, &mut dst)
        }));

        match result {
            Ok(Ok(())) => {
                self.capture_accum.drain(..block_len);
                let interleaved = interleave(
                    &self.capture_out,
                    self.capture_channels,
                    self.capture_block_frames,
                );
                self.next_capture_pts = Some(base_pts.saturating_add(block_frames_i64));
                Ok(Some(self.raw_capture_frame(base_pts, &interleaved)))
            }
            Ok(Err(err)) => Err(ApmError::Backend(err)),
            Err(_) => {
                self.inner = None;
                Err(ApmError::BackendPanicked)
            }
        }
    }

    /// Builds one output [`AudioFrame`] from interleaved capture-format
    /// samples starting at `pts`.
    fn raw_capture_frame(&self, pts: i64, interleaved: &[f32]) -> AudioFrame {
        AudioFrame {
            pts,
            duration: u64::try_from(self.capture_block_frames).unwrap_or(u64::MAX),
            sample_rate: self.capture_format.sample_rate,
            channels: self.capture_format.channels,
            format: SampleFormat::F32,
            data: f32_to_bytes(interleaved),
        }
    }

    /// Estimated render→capture round-trip delay (echo-alignment hint).
    /// No-op once [`is_disabled`](Self::is_disabled). Not wrapped in
    /// `catch_unwind` — not one of this crate's documented panic-safety
    /// call sites (`build`/`process_render_f32`/`process_capture_f32`/
    /// `analyze`); `sonora`'s own clamping-to-range error is intentionally
    /// discarded here, matching this method's infallible signature.
    pub fn set_stream_delay_ms(&mut self, ms: i32) {
        if let Some(inner) = self.inner.as_mut() {
            let _ = inner.set_stream_delay_ms(ms);
        }
    }

    /// `true` after a caught backend panic — this instance now passes
    /// frames through unmodified for its remaining lifetime. Construct a
    /// new instance to retry.
    #[must_use]
    pub const fn is_disabled(&self) -> bool {
        self.inner.is_none()
    }
}

#[cfg(test)]
#[path = "processor_tests.rs"]
mod tests;
