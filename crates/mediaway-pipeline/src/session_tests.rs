#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;
use crate::filter::{FilterError, FrameFilter};
use mediaway_common::{
    Bytes, CodecKind, GpuBufferHandle, Packet, PixelFormat, Rational, SampleFormat, StreamInfo,
    VideoFrameStorage, VideoGeometry,
};
use mediaway_encoder::EncodeError;
use mediaway_sw::apm::{ApmConfig, AudioProcessor, VoiceActivityDetector};
use std::cell::Cell;
use std::rc::Rc;

const AUDIO_SAMPLE_RATE: u32 = 8_000;
const AUDIO_CHANNELS: u16 = 1;
/// Samples per 10ms block at [`AUDIO_SAMPLE_RATE`] — matches
/// `mediaway-audio-apm/src/processor_tests.rs`'s own convention.
const AUDIO_BLOCK: usize = (AUDIO_SAMPLE_RATE / 100) as usize;

/// Minimal [`AudioEncoder`] that records every frame it receives instead of actually
/// encoding — the audio-side counterpart to [`MockEncoder`]. Records into a shared
/// `Rc<RefCell<..>>` (rather than a plain `Vec` field) so a test can keep observing
/// pushed frames after the encoder itself has moved into an opaque `Box<dyn
/// AudioEncoder>` inside `EncodeSession` — mirrors this file's existing
/// `Rc<Cell<usize>>` pattern for `FrameFilter` call counts.
struct MockAudioEncoder {
    info: StreamInfo,
    pushed: Rc<std::cell::RefCell<Vec<AudioFrame>>>,
}

impl MockAudioEncoder {
    fn new(pushed: Rc<std::cell::RefCell<Vec<AudioFrame>>>) -> Self {
        Self {
            info: StreamInfo::Audio {
                id: 0,
                codec: CodecKind::Aac,
                time_base: Rational::new(1, AUDIO_SAMPLE_RATE),
                extra_data: Bytes::new(),
                sample_rate: AUDIO_SAMPLE_RATE,
                channels: AUDIO_CHANNELS,
            },
            pushed,
        }
    }
}

impl mediaway_encoder::AudioEncoder for MockAudioEncoder {
    fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    fn push_frame(&mut self, frame: &AudioFrame) -> Result<(), EncodeError> {
        self.pushed.borrow_mut().push(frame.clone());
        Ok(())
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        Ok(None)
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        Ok(())
    }
}

fn f32_bytes(samples: &[f32]) -> Bytes {
    let mut buf = Vec::with_capacity(samples.len() * 4);
    for &sample in samples {
        buf.extend_from_slice(&sample.to_le_bytes());
    }
    Bytes::from(buf)
}

fn audio_frame(pts: i64, samples: &[f32]) -> AudioFrame {
    AudioFrame {
        pts,
        duration: samples.len() as u64,
        sample_rate: AUDIO_SAMPLE_RATE,
        channels: AUDIO_CHANNELS,
        format: SampleFormat::F32,
        data: f32_bytes(samples),
    }
}

/// All-components-disabled config — deterministic passthrough, no `sonora` DSP cost.
fn open_processor() -> AudioProcessor {
    let format = mediaway_sw::apm::AudioStreamFormat {
        sample_rate: AUDIO_SAMPLE_RATE,
        channels: AUDIO_CHANNELS,
        sample_format: SampleFormat::F32,
    };
    AudioProcessor::open(ApmConfig::default(), format, format).expect("open processor")
}

/// Minimal [`VideoEncoder`] that records every frame it receives instead of
/// actually encoding — enough to observe what [`EncodeSession::write_frame`]
/// hands to the encoder without a real platform backend.
struct MockEncoder {
    info: StreamInfo,
    pushed: Vec<VideoFrame>,
}

impl MockEncoder {
    fn new() -> Self {
        Self {
            info: StreamInfo::Video {
                id: 0,
                codec: CodecKind::H264,
                time_base: Rational::new(1, 30),
                geometry: VideoGeometry {
                    width: 4,
                    height: 4,
                },
                extra_data: Bytes::new(),
            },
            pushed: Vec::new(),
        }
    }
}

impl VideoEncoder for MockEncoder {
    fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    fn push_frame(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        self.pushed.push(frame.clone());
        Ok(())
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        Ok(None)
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        Ok(())
    }
}

fn cpu_frame(pts: i64) -> VideoFrame {
    VideoFrame {
        pts,
        duration: 1,
        width: 4,
        height: 4,
        format: PixelFormat::I420,
        storage: VideoFrameStorage::Cpu {
            data: Bytes::from_static(&[0u8; 24]),
        },
    }
}

fn gpu_frame(pts: i64) -> VideoFrame {
    VideoFrame {
        pts,
        duration: 1,
        width: 4,
        height: 4,
        format: PixelFormat::I420,
        storage: VideoFrameStorage::Gpu(GpuBufferHandle::WebGpu { texture_id: 1 }),
    }
}

/// Filter that adds a fixed offset to `pts` and counts how many times it ran
/// (via a shared cell so the test can inspect it after `push_filter` moves
/// the filter into the session's chain).
struct PtsOffsetFilter {
    offset: i64,
    calls: Rc<Cell<usize>>,
}

impl FrameFilter for PtsOffsetFilter {
    fn process(&mut self, mut frame: VideoFrame) -> Result<VideoFrame, FilterError> {
        self.calls.set(self.calls.get() + 1);
        frame.pts += self.offset;
        Ok(frame)
    }
}

/// Filter that always rejects, counting how many times it ran.
struct RejectingFilter {
    calls: Rc<Cell<usize>>,
}

impl FrameFilter for RejectingFilter {
    fn process(&mut self, _frame: VideoFrame) -> Result<VideoFrame, FilterError> {
        self.calls.set(self.calls.get() + 1);
        Err(FilterError::Rejected)
    }
}

#[test]
fn write_frame_with_empty_chain_pushes_frame_unchanged() {
    let mut session = EncodeSession::open(MockEncoder::new()).expect("open session");

    let frame = cpu_frame(10);
    session.write_frame(&frame).expect("write frame");

    assert_eq!(session.encoder.pushed, vec![frame]);
}

#[test]
fn push_filter_runs_stateful_filter_before_encoder() {
    let mut session = EncodeSession::open(MockEncoder::new()).expect("open session");
    let calls = Rc::new(Cell::new(0));
    session.push_filter(PtsOffsetFilter {
        offset: 100,
        calls: Rc::clone(&calls), // clone: share the counter with the test assertion below
    });

    session.write_frame(&cpu_frame(5)).expect("write frame");

    assert_eq!(calls.get(), 1);
    assert_eq!(session.encoder.pushed.len(), 1);
    assert_eq!(session.encoder.pushed[0].pts, 105);
}

#[test]
fn filter_rejection_aborts_write_frame() {
    let mut session = EncodeSession::open(MockEncoder::new()).expect("open session");
    let calls = Rc::new(Cell::new(0));
    session.push_filter(RejectingFilter {
        calls: Rc::clone(&calls), // clone: share the counter with the test assertion below
    });

    let result = session.write_frame(&cpu_frame(1));

    assert_eq!(calls.get(), 1);
    assert!(session.encoder.pushed.is_empty());
    assert!(matches!(
        result,
        Err(PipelineError::Filter(FilterError::Rejected))
    ));
}

#[test]
fn gpu_frame_with_filters_returns_unsupported_without_running_filter() {
    let mut session = EncodeSession::open(MockEncoder::new()).expect("open session");
    let calls = Rc::new(Cell::new(0));
    session.push_filter(PtsOffsetFilter {
        offset: 1,
        calls: Rc::clone(&calls), // clone: share the counter with the test assertion below
    });

    let result = session.write_frame(&gpu_frame(1));

    assert_eq!(calls.get(), 0, "filter chain must not run on Gpu frames");
    assert!(session.encoder.pushed.is_empty());
    assert!(matches!(
        result,
        Err(PipelineError::Filter(FilterError::GpuFrameUnsupported))
    ));
}

#[test]
fn write_audio_frame_without_audio_track_is_no_audio_track_error() {
    let mut session = EncodeSession::open(MockEncoder::new()).expect("open session");
    let result = session.write_audio_frame(&audio_frame(0, &[0.0; AUDIO_BLOCK]));
    assert!(matches!(result, Err(PipelineError::NoAudioTrack)));
}

#[test]
fn attach_audio_processor_without_audio_track_is_no_audio_track_error() {
    let mut session = EncodeSession::open(MockEncoder::new()).expect("open session");
    let result = session.attach_audio_processor(open_processor());
    assert!(matches!(result, Err(PipelineError::NoAudioTrack)));
}

#[test]
fn attach_vad_without_audio_track_is_no_audio_track_error() {
    let mut session = EncodeSession::open(MockEncoder::new()).expect("open session");
    let vad = VoiceActivityDetector::open(AUDIO_SAMPLE_RATE).expect("open vad");
    let result = session.attach_vad(vad);
    assert!(matches!(result, Err(PipelineError::NoAudioTrack)));
}

#[test]
fn write_audio_frame_without_processor_pushes_straight_to_encoder() {
    let pushed = Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut session = EncodeSession::open_with_audio(
        MockEncoder::new(),
        MockAudioEncoder::new(Rc::clone(&pushed)),
    )
    .expect("open session with audio");

    let frame = audio_frame(0, &[0.5; AUDIO_BLOCK]);
    session
        .write_audio_frame(&frame)
        .expect("write audio frame");

    assert_eq!(*pushed.borrow(), vec![frame]);
}

#[test]
fn write_audio_frame_with_processor_reblocks_and_scores_vad() {
    let pushed = Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut session = EncodeSession::open_with_audio(
        MockEncoder::new(),
        MockAudioEncoder::new(Rc::clone(&pushed)),
    )
    .expect("open session with audio");
    session
        .attach_audio_processor(open_processor())
        .expect("attach processor");
    let vad = VoiceActivityDetector::open(AUDIO_SAMPLE_RATE).expect("open vad");
    session.attach_vad(vad).expect("attach vad");

    // No score/output yet — less than one full 10ms block accumulated.
    session
        .write_audio_frame(&audio_frame(0, &[0.0; AUDIO_BLOCK / 2]))
        .expect("write partial block");
    assert_eq!(session.poll_vad_score(), None);

    // Completes one block — one processed frame reaches the encoder, one VAD score.
    session
        .write_audio_frame(&audio_frame(0, &[0.0; AUDIO_BLOCK / 2]))
        .expect("write remaining half");

    assert_eq!(
        pushed.borrow().len(),
        1,
        "one full 10ms block should reach the encoder"
    );
    assert_eq!(pushed.borrow()[0].data.len(), AUDIO_BLOCK * 4);

    let score = session.poll_vad_score();
    assert!(score.is_some(), "one VAD score should be queued");
    assert!((0.0..=1.0).contains(&score.expect("score")));
    assert_eq!(session.poll_vad_score(), None, "queue drained");
}

#[test]
fn write_audio_render_frame_without_processor_is_a_no_op() {
    let pushed = Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut session =
        EncodeSession::open_with_audio(MockEncoder::new(), MockAudioEncoder::new(pushed))
            .expect("open session with audio");
    session
        .write_audio_render_frame(&audio_frame(0, &[0.0; AUDIO_BLOCK]))
        .expect("no-op without an attached processor");
}

#[test]
fn write_audio_render_frame_without_audio_track_is_no_audio_track_error() {
    let mut session = EncodeSession::open(MockEncoder::new()).expect("open session");
    let result = session.write_audio_render_frame(&audio_frame(0, &[0.0; AUDIO_BLOCK]));
    assert!(matches!(result, Err(PipelineError::NoAudioTrack)));
}
