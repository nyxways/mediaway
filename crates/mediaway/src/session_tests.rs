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
    last_bitrate: Option<u32>,
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
            last_bitrate: None,
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

    fn set_bitrate(&mut self, bitrate_bps: u32) -> Result<(), EncodeError> {
        self.last_bitrate = Some(bitrate_bps);
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
fn set_bitrate_forwards_to_the_underlying_encoder() {
    let mut session = EncodeSession::open(MockEncoder::new()).expect("open session");

    session.set_bitrate(2_000_000).expect("set bitrate");

    assert_eq!(session.encoder.last_bitrate, Some(2_000_000));
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

/// Minimal [`VideoEncoder`] that emits one packet per pushed frame.
///
/// [`MockEncoder`] never emits any, so nothing reaches the muxer through it and there are
/// no container bytes to poll — which is exactly what the byte-output tests below need to
/// observe. Payloads are deterministic (derived from `pts`) so two sessions fed the same
/// frames produce byte-identical output, which is what
/// [`finish_into`](EncodeSession::finish_into)'s "only what was not polled" contract is
/// checked against.
struct PacketEmittingEncoder {
    info: StreamInfo,
    queued: VecDeque<Packet>,
}

impl PacketEmittingEncoder {
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
                extra_data: Bytes::from_static(&[0x01, 0x64, 0x00, 0x1f]),
            },
            queued: VecDeque::new(),
        }
    }
}

impl VideoEncoder for PacketEmittingEncoder {
    fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    fn push_frame(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        let byte = u8::try_from(frame.pts.rem_euclid(256)).unwrap_or(0);
        self.queued.push_back(Packet {
            stream_id: 0,
            pts: frame.pts,
            dts: frame.pts,
            duration: 1,
            is_keyframe: frame.pts == 0,
            is_discard: false,
            payload: Bytes::from(vec![byte; 32]),
        });
        Ok(())
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        Ok(self.queued.pop_front())
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        Ok(())
    }
}

/// Enough frames to cross `iso_bmff::DEFAULT_FRAGMENT_BATCH` (30) so the muxer emits a
/// complete fragment mid-session, not just the init segment.
const FRAMES_PAST_ONE_FRAGMENT: i64 = 35;

#[test]
fn poll_bytes_yields_nothing_before_any_frame_is_written() {
    let mut session = EncodeSession::open(PacketEmittingEncoder::new()).expect("open session");

    let mut out = Vec::new();
    assert_eq!(session.poll_bytes(&mut out), 0);
    assert!(out.is_empty());
}

#[test]
fn poll_bytes_streams_container_bytes_before_finish() {
    let mut session = EncodeSession::open(PacketEmittingEncoder::new()).expect("open session");

    for pts in 0..FRAMES_PAST_ONE_FRAGMENT {
        session.write_frame(&cpu_frame(pts)).expect("write frame");
    }

    let mut out = Vec::new();
    let written = session.poll_bytes(&mut out);
    assert!(
        written > 0,
        "a fragment's worth of frames must be readable without finishing the session"
    );
    assert_eq!(written, out.len(), "poll_bytes returns what it appended");
}

#[test]
fn poll_bytes_appends_rather_than_overwriting() {
    let mut session = EncodeSession::open(PacketEmittingEncoder::new()).expect("open session");
    for pts in 0..FRAMES_PAST_ONE_FRAGMENT {
        session.write_frame(&cpu_frame(pts)).expect("write frame");
    }

    let mut out = vec![0xAB, 0xCD];
    let written = session.poll_bytes(&mut out);

    assert_eq!(&out[..2], &[0xAB, 0xCD], "existing content is preserved");
    assert_eq!(out.len(), 2 + written);
}

#[test]
fn polling_then_finishing_produces_the_same_stream_as_finishing_alone() {
    let mut streamed = Vec::new();
    let mut session = EncodeSession::open(PacketEmittingEncoder::new()).expect("open session");
    for pts in 0..FRAMES_PAST_ONE_FRAGMENT {
        session.write_frame(&cpu_frame(pts)).expect("write frame");
        session.poll_bytes(&mut streamed);
    }
    let tail = session
        .finish_into(&mut streamed)
        .expect("finish into the same buffer");
    assert!(tail > 0, "the trailing fragment only appears at finish");

    let mut whole = EncodeSession::open(PacketEmittingEncoder::new()).expect("open session");
    for pts in 0..FRAMES_PAST_ONE_FRAGMENT {
        whole.write_frame(&cpu_frame(pts)).expect("write frame");
    }
    let whole = whole.finish().expect("finish");

    assert_eq!(
        streamed, whole,
        "incremental polling must not change the bytes, only when they arrive"
    );
}

#[test]
fn finish_after_polling_returns_only_the_unpolled_tail() {
    let mut session = EncodeSession::open(PacketEmittingEncoder::new()).expect("open session");
    for pts in 0..FRAMES_PAST_ONE_FRAGMENT {
        session.write_frame(&cpu_frame(pts)).expect("write frame");
    }
    let mut polled = Vec::new();
    session.poll_bytes(&mut polled);
    assert!(!polled.is_empty(), "the first fragment was drained");

    let tail = session.finish().expect("finish");

    assert!(!tail.is_empty(), "the flushed remainder is still returned");
    assert_eq!(
        &polled[4..8],
        b"ftyp",
        "the init segment went to the caller that polled for it"
    );
    assert!(
        !tail.windows(4).any(|w| w == b"ftyp"),
        "finish() returns the tail, not a second copy of the whole stream — see ADR-0006"
    );
}
