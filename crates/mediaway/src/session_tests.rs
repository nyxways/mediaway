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
        Self::with_codec(
            CodecKind::H264,
            Bytes::from_static(&[0x01, 0x64, 0x00, 0x1f]),
        )
    }

    /// [`Self::new`] for a chosen codec — `WebM` has no `CodecID` for H.264, so a
    /// container-choice test needs a stream the other container can actually carry.
    fn with_codec(codec: CodecKind, extra_data: Bytes) -> Self {
        Self {
            info: StreamInfo::Video {
                id: 0,
                codec,
                time_base: Rational::new(1, 30),
                geometry: VideoGeometry {
                    width: 4,
                    height: 4,
                },
                extra_data,
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

/// `EncodeSession` is generic over its muxer (`adr/0007-encode-session-generic-muxer.md`).
/// These are the tests that would fail if it compiled but still wrote MP4 regardless of the
/// muxer handed to it — the failure mode a type-level change like this invites.
mod container_choice {
    use super::{FRAMES_PAST_ONE_FRAGMENT, MockAudioEncoder, PacketEmittingEncoder, cpu_frame};
    use crate::{EncodeSession, PipelineError};
    use mediaway_common::{Bytes, CodecKind, StreamInfo};
    use mediaway_container::{ContainerError, MuxOpen, mp4, webm};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn vp9_encoder() -> PacketEmittingEncoder {
        // VP9 carries no out-of-band configuration record, so an empty `extra_data` here
        // is correct rather than a shortcut — it also keeps `drain`'s late-`extra_data`
        // backfill out of the picture, which WebM could not honour anyway.
        PacketEmittingEncoder::with_codec(CodecKind::Vp9, Bytes::new())
    }

    fn encode_all<M>(mut session: EncodeSession<PacketEmittingEncoder, M>) -> Vec<u8>
    where
        M: MuxOpen,
        M::Error: Into<ContainerError>,
    {
        for pts in 0..FRAMES_PAST_ONE_FRAGMENT {
            session.write_frame(&cpu_frame(pts)).expect("write frame");
        }
        session.finish().expect("finish session")
    }

    #[test]
    fn a_webm_session_produces_bytes_the_webm_demuxer_can_read_back() {
        let session =
            EncodeSession::open_in(webm::Muxer::new(), vp9_encoder()).expect("open webm session");
        let bytes = encode_all(session);

        // Round-tripping through the real demuxer, rather than sniffing the EBML magic,
        // is what actually proves the packets went into *this* container: a byte-prefix
        // check would still pass on a file whose header is WebM and whose body is empty.
        let mut demuxer = webm::Demuxer::new();
        demuxer.push_bytes(&bytes);
        let mut packets = 0;
        while demuxer.poll_packet().is_some() {
            packets += 1;
        }

        let streams = demuxer.streams();
        assert_eq!(streams.len(), 1, "expected exactly one muxed track");
        assert!(
            matches!(
                streams[0],
                StreamInfo::Video {
                    codec: CodecKind::Vp9,
                    ..
                }
            ),
            "expected a VP9 video track, got {:?}",
            streams[0]
        );
        assert_eq!(
            i64::from(packets),
            FRAMES_PAST_ONE_FRAGMENT,
            "every encoded packet should survive the round trip"
        );
    }

    #[test]
    fn the_default_session_still_produces_mp4() {
        // The regression guard for the default type parameter: `open` must keep meaning
        // fragmented MP4 for every caller that never asked for anything else.
        let session = EncodeSession::open(PacketEmittingEncoder::new()).expect("open session");
        let bytes = encode_all(session);

        let mut demuxer = mp4::Demuxer::new();
        demuxer.push_bytes(&bytes);
        assert_eq!(demuxer.streams().len(), 1, "expected one MP4 track");
        assert!(
            matches!(
                demuxer.streams()[0],
                StreamInfo::Video {
                    codec: CodecKind::H264,
                    ..
                }
            ),
            "expected an H.264 video track"
        );
    }

    #[test]
    fn opening_a_webm_session_rejects_a_codec_webm_cannot_carry() {
        // H.264 has no WebM `CodecID`. The mismatch has to surface at `open_in`, because
        // the alternative — accepting the track and writing a file no player can decode —
        // is the failure this whole seam is supposed to make impossible.
        let err = EncodeSession::open_in(webm::Muxer::new(), PacketEmittingEncoder::new())
            .err()
            .expect("H.264 in WebM must be rejected");

        assert!(
            matches!(
                err,
                PipelineError::Mux(ContainerError::Webm(webm::Error::UnsupportedCodec(
                    CodecKind::H264
                )))
            ),
            "expected an UnsupportedCodec mux error, got {err:?}"
        );
    }

    #[test]
    fn a_two_track_webm_session_numbers_tracks_from_the_containers_first_id() {
        // `open_with_audio` used to hardcode video `0` / audio `1`. On WebM that is an
        // immediate `InvalidTrackNumber`, so the pair has to start at the container's own
        // floor — this is the test that pins `MuxOpen::FIRST_TRACK_ID` to real behaviour
        // rather than leaving it a constant nothing reads.
        let session = EncodeSession::open_in_with_audio(
            webm::Muxer::new(),
            vp9_encoder(),
            MockAudioEncoder::new(Rc::new(RefCell::new(Vec::new()))),
        )
        .expect("open two-track webm session");

        let bytes = encode_all(session);
        let mut demuxer = webm::Demuxer::new();
        demuxer.push_bytes(&bytes);

        let ids: Vec<u32> = demuxer.streams().iter().map(StreamInfo::id).collect();
        assert_eq!(
            ids,
            vec![
                <webm::Muxer as MuxOpen>::FIRST_TRACK_ID,
                <webm::Muxer as MuxOpen>::FIRST_TRACK_ID + 1
            ],
            "WebM tracks must start at 1, not at the encoders' default 0"
        );
    }

    #[test]
    fn open_in_reaches_muxer_options_the_facade_does_not_mirror() {
        // `mp4::Muxer::with_fragment_batch` was unreachable through `EncodeSession` before
        // `open_in` existed. A batch of 2 must emit a complete fragment well before the
        // default batch of 30 does; without that difference, `open_in` is taking the
        // caller's muxer and throwing its configuration away.
        const FRAMES: i64 = 6;

        let mut small = EncodeSession::open_in(
            mp4::Muxer::with_fragment_batch(2),
            PacketEmittingEncoder::new(),
        )
        .expect("open small-batch session");
        let mut default = EncodeSession::open(PacketEmittingEncoder::new()).expect("open session");

        let (mut small_bytes, mut default_bytes) = (Vec::new(), Vec::new());
        for pts in 0..FRAMES {
            small.write_frame(&cpu_frame(pts)).expect("write frame");
            default.write_frame(&cpu_frame(pts)).expect("write frame");
        }
        small.poll_bytes(&mut small_bytes);
        default.poll_bytes(&mut default_bytes);

        assert!(
            small_bytes.len() > default_bytes.len(),
            "a 2-frame fragment batch should have flushed more than the 30-frame default \
             after {FRAMES} frames, got {} vs {}",
            small_bytes.len(),
            default_bytes.len()
        );
    }
}
