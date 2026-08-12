//! iOS screen capture via `ReplayKit`: `RPScreenRecorder` in-app capture
//! ([`AppleScreenCapture`]) **and** a Broadcast Upload Extension push-sink
//! ([`AppleBroadcastExtensionCapture`]). See
//! [ADR-0004](adr/apple/0004-replaykit-ios-inapp-screen-capture.md).
//!
//! Both entry points share one dispatch routine, [`classify_and_queue_sample_buffer`], and the
//! same `CVPixelBuffer` extraction ([`super::pixel::extract_nv12`]) `apple::camera`/
//! `apple::screencapturekit` already use — no per-entry-point-duplicated frame extraction.
//!
//! Both entry points implement three traits — [`DesktopVideoCapture`] (screen),
//! [`DesktopAudioCapture`] (app audio — "what this app/broadcast is playing"), and
//! [`crate::audio::AudioCapture`] (microphone audio) — since a single `ReplayKit` stream
//! delivers all three tagged by [`RPSampleBufferType`]. See ADR-0004 § Audio inclusion for why
//! `AudioApp`/`AudioMic` map to these two different traits rather than one.
//!
//! `AppleBroadcastExtensionCapture` has **no OS session of its own** — unlike every other
//! backend in this crate, it does not `open()` an OS capture session; the host project's own
//! `.appex` extension target (a genuine Xcode project-structure requirement this crate cannot
//! build or ship) owns the real `RPBroadcastSampleHandler` lifecycle and calls
//! [`AppleBroadcastExtensionCapture::push_sample_buffer`] once per
//! `processSampleBuffer:withType:` invocation — see ADR-0004 § Host-extension contract for the
//! full, real Swift/Objective-C contract this crate cannot fulfill itself.
//!
//! **Zero compile verification, zero real-hardware verification** — see the crate's `apple-ios`
//! CI job and ADR-0004 § "A real, honest verification-gap note" (this domain's ceiling is lower
//! than every other backend's: no CI configuration in this workspace can exercise a real
//! `.appex`/`RPBroadcastSampleHandler` integration).

#![allow(unsafe_code)]

use std::collections::VecDeque;
use std::ffi::c_char;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::CaptureError;
use crate::audio::AudioCapture;
use crate::desktop::DesktopAudioCapture;
use crate::desktop::DesktopVideoCapture;
use block2::RcBlock;
use mediaway_common::{
    AudioFrame, Bytes, CodecKind, PixelFormat, Rational, SampleFormat, StreamInfo, VideoFrame,
    VideoFrameStorage, VideoGeometry,
};
use objc2::rc::Retained;
use objc2_core_audio_types::{kAudioFormatFlagIsFloat, kAudioFormatFlagIsNonInterleaved};
use objc2_core_media::{CMAudioFormatDescription, CMSampleBuffer};
use objc2_foundation::NSError;
use objc2_replay_kit::{RPSampleBufferType, RPScreenRecorder};

/// Bounded, drop-oldest queue depth per data kind — mirrors every other backend in this crate.
const QUEUE_CAP: usize = 4;

/// How long `open()`/`close()` wait for `RPScreenRecorder`'s completion handlers — no real
/// device this session to tune against (ADR-0004 § Open questions, shared with ADR-0003's own
/// completion-handler timeout question).
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(10);

struct FrameQueues {
    video: Mutex<VecDeque<VideoFrame>>,
    app_audio: Mutex<VecDeque<AudioFrame>>,
    mic_audio: Mutex<VecDeque<AudioFrame>>,
}

impl FrameQueues {
    fn new() -> Self {
        Self {
            video: Mutex::new(VecDeque::new()),
            app_audio: Mutex::new(VecDeque::new()),
            mic_audio: Mutex::new(VecDeque::new()),
        }
    }
}

struct StreamInfos {
    /// Set once, from the first frame of its kind — video geometry / audio format do not
    /// change mid-session in practice, so "first frame wins" avoids the awkward lifetime shape
    /// a `Mutex<StreamInfo>` would force on `stream_info()`'s `&StreamInfo` return type.
    video: std::sync::OnceLock<StreamInfo>,
    app_audio: std::sync::OnceLock<StreamInfo>,
    mic_audio: std::sync::OnceLock<StreamInfo>,
}

impl StreamInfos {
    fn new() -> Self {
        Self {
            video: std::sync::OnceLock::new(),
            app_audio: std::sync::OnceLock::new(),
            mic_audio: std::sync::OnceLock::new(),
        }
    }
}

/// Placeholder returned by `stream_info()` before the first frame of a kind has arrived, or
/// after the session is closed — mirrors every other backend's `closed_video_info` shape.
fn unknown_video_info() -> &'static StreamInfo {
    use std::sync::OnceLock;
    static INFO: OnceLock<StreamInfo> = OnceLock::new();
    INFO.get_or_init(|| StreamInfo::Video {
        id: 0,
        codec: CodecKind::RawVideo,
        time_base: Rational::new(1, 30),
        geometry: VideoGeometry {
            width: 0,
            height: 0,
        },
        extra_data: Bytes::new(),
    })
}

/// Placeholder returned by `stream_info()` before the first frame of a kind has arrived, or
/// after the session is closed — mirrors every other backend's `closed_audio_info` shape.
fn unknown_audio_info() -> &'static StreamInfo {
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

/// Dispatches one `(CMSampleBuffer, RPSampleBufferType)` delivery to the right queue —
/// [`AppleScreenCapture`]'s in-app `block2` closure and
/// [`AppleBroadcastExtensionCapture::push_sample_buffer`] both call this, per ADR-0004's "one
/// shared dispatch helper" decision.
///
/// # Safety
///
/// `sample_buffer` must be a valid, non-null `CMSampleBuffer` for the duration of this call —
/// the caller's own `ReplayKit` callback/extension contract.
unsafe fn classify_and_queue_sample_buffer(
    queues: &FrameQueues,
    infos: &StreamInfos,
    next_pts: &AtomicI64,
    sample_buffer: &CMSampleBuffer,
    kind: RPSampleBufferType,
) {
    match kind {
        RPSampleBufferType::Video => {
            // SAFETY: caller's contract (this fn's own `# Safety`).
            let Some((data, width, height)) =
                (unsafe { super::pixel::extract_nv12(sample_buffer) })
            else {
                return;
            };
            let pts = next_pts.fetch_add(1, Ordering::Relaxed);
            let _ = infos.video.set(StreamInfo::Video {
                id: 0,
                codec: CodecKind::RawVideo,
                time_base: Rational::new(1, 30),
                geometry: VideoGeometry { width, height },
                extra_data: Bytes::new(),
            });
            let frame = VideoFrame {
                pts,
                duration: 1,
                width,
                height,
                format: PixelFormat::Nv12,
                storage: VideoFrameStorage::Cpu { data },
            };
            push_bounded(&queues.video, frame);
        }
        RPSampleBufferType::AudioApp => {
            // SAFETY: caller's contract.
            if let Some(frame) = unsafe { extract_pcm(sample_buffer) } {
                let _ = infos.app_audio.set(audio_stream_info(&frame));
                push_bounded(&queues.app_audio, frame);
            }
        }
        RPSampleBufferType::AudioMic => {
            // SAFETY: caller's contract.
            if let Some(frame) = unsafe { extract_pcm(sample_buffer) } {
                let _ = infos.mic_audio.set(audio_stream_info(&frame));
                push_bounded(&queues.mic_audio, frame);
            }
        }
        _ => {}
    }
}

fn audio_stream_info(frame: &AudioFrame) -> StreamInfo {
    StreamInfo::Audio {
        id: 0,
        codec: CodecKind::RawAudio,
        time_base: Rational::new(1, frame.sample_rate.max(1)),
        sample_rate: frame.sample_rate,
        channels: frame.channels,
        extra_data: Bytes::new(),
    }
}

fn push_bounded<T>(queue: &Mutex<VecDeque<T>>, item: T) {
    if let Ok(mut q) = queue.lock() {
        if q.len() >= QUEUE_CAP {
            let _ = q.pop_front();
        }
        q.push_back(item);
    }
}

/// Extract one interleaved `F32` PCM [`AudioFrame`] from `sample_buffer`'s `CMBlockBuffer` +
/// `CMAudioFormatDescription`. Returns `None` for anything not provably interleaved `F32` PCM —
/// never silently mis-reads a layout it didn't verify.
///
/// # Safety
///
/// `sample_buffer` must be a valid, non-null `CMSampleBuffer` for the duration of this call.
unsafe fn extract_pcm(sample_buffer: &CMSampleBuffer) -> Option<AudioFrame> {
    // SAFETY: caller's contract.
    let format_description = unsafe { sample_buffer.format_description() }?;
    let audio_description = format_description
        .downcast::<CMAudioFormatDescription>()
        .ok()?;
    // `CMAudioFormatDescription` has no inherent `stream_basic_description` method — the real API
    // is the free function `CMAudioFormatDescriptionGetStreamBasicDescription`.
    //
    // SAFETY: `audio_description` is a valid, just-downcast format description.
    let asbd_ptr = unsafe {
        objc2_core_media::CMAudioFormatDescriptionGetStreamBasicDescription(&audio_description)
    };
    if asbd_ptr.is_null() {
        return None;
    }
    // SAFETY: `asbd_ptr` is non-null (checked above); `CMAudioFormatDescriptionGetStreamBasicDescription`'s
    // documented contract guarantees it points to a valid, readable `AudioStreamBasicDescription`
    // for the format description's own lifetime, which outlives this read.
    let asbd = unsafe { *asbd_ptr };
    if asbd.mFormatFlags & kAudioFormatFlagIsFloat == 0 {
        return None;
    }
    if asbd.mFormatFlags & kAudioFormatFlagIsNonInterleaved != 0 {
        return None;
    }
    let channels = asbd.mChannelsPerFrame;
    if channels == 0 || !(asbd.mSampleRate > 0.0) {
        return None;
    }

    // SAFETY: caller's contract.
    let block_buffer = unsafe { sample_buffer.data_buffer() }?;
    // SAFETY: `block_buffer` is a valid, just-obtained `CMBlockBuffer`.
    let total_len = unsafe { block_buffer.data_length() };
    let mut length_at_offset: usize = 0;
    let mut data_ptr: *mut c_char = std::ptr::null_mut();
    // `data_pointer`'s real signature takes raw out-parameter pointers (`*mut usize`,
    // `*mut *mut c_char`), not `Option<&mut T>` — a null pointer means "don't care" for
    // `total_length_out`.
    //
    // SAFETY: `block_buffer` is valid; `length_at_offset`/`data_ptr` are valid local out-params,
    // each a valid pointer for the call's duration.
    let status = unsafe {
        block_buffer.data_pointer(
            0,
            &raw mut length_at_offset,
            std::ptr::null_mut(),
            &raw mut data_ptr,
        )
    };
    if status != 0 || data_ptr.is_null() || length_at_offset != total_len {
        // Not a single contiguous range — never silently mis-read a discontiguous buffer.
        return None;
    }
    // SAFETY: `data_ptr` is non-null and readable for `total_len` bytes — the documented
    // `CMBlockBufferGetDataPointer` contract, checked (`length_at_offset == total_len`) above.
    let bytes = unsafe { std::slice::from_raw_parts(data_ptr.cast::<u8>(), total_len) };

    let bytes_per_frame = 4usize.saturating_mul(channels as usize);
    let num_frames = if bytes_per_frame == 0 {
        0
    } else {
        total_len / bytes_per_frame
    };

    // `asbd.mSampleRate > 0.0` is checked above; real audio sample rates (e.g. 44100/48000) are
    // always small positive integers, exact in `u32`.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "mSampleRate > 0.0 checked above; real sample rates are small positive integers"
    )]
    let sample_rate = asbd.mSampleRate as u32;

    Some(AudioFrame {
        pts: 0,
        duration: u64::try_from(num_frames).unwrap_or(0),
        sample_rate,
        channels: u16::try_from(channels).unwrap_or(0),
        format: SampleFormat::F32,
        data: Bytes::copy_from_slice(bytes),
    })
}

// ───────────────────────── AppleScreenCapture (in-app, RPScreenRecorder) ─────────────────────────

struct InAppSession {
    queues: Arc<FrameQueues>,
    infos: Arc<StreamInfos>,
    recorder: Retained<RPScreenRecorder>,
    // Kept alive for the whole session — `startCaptureWithHandler` retains its own reference,
    // but this crate's own convention (mirrors every other delegate/callback backend) is to
    // hold the block alive explicitly too.
    //
    // `RcBlock<F>` takes exactly one generic parameter — the `dyn Fn(...)` block signature itself
    // — not a separate lifetime/fn-pointer/marker-trait triple.
    _capture_handler:
        RcBlock<dyn Fn(std::ptr::NonNull<CMSampleBuffer>, RPSampleBufferType, *mut NSError)>,
}

/// iOS in-app screen capture (`RPScreenRecorder`), including app audio and microphone audio.
/// See module docs for the trait split.
pub struct AppleScreenCapture {
    inner: Option<InAppSession>,
}

impl AppleScreenCapture {
    /// Open `RPScreenRecorder` in-app capture. Always captures video + app audio + microphone
    /// audio (confirmed with the user — ADR-0004 § Decisions confirmed).
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Backend`] on `ReplayKit` failure or completion-handler timeout.
    pub fn open() -> Result<Self, CaptureError> {
        // SAFETY: plain, always-safe-to-call singleton accessor.
        let recorder = unsafe { RPScreenRecorder::sharedRecorder() };
        // SAFETY: `recorder` is the valid shared singleton.
        unsafe { recorder.setMicrophoneEnabled(true) };

        let queues = Arc::new(FrameQueues::new());
        let infos = Arc::new(StreamInfos::new());
        let next_pts = Arc::new(AtomicI64::new(0));
        // clone: capture handler closure needs its own strong refs to push frames
        let queues_cb = Arc::clone(&queues);
        let infos_cb = Arc::clone(&infos);
        let next_pts_cb = Arc::clone(&next_pts);

        let capture_handler: RcBlock<
            dyn Fn(std::ptr::NonNull<CMSampleBuffer>, RPSampleBufferType, *mut NSError),
        > = RcBlock::new(
            move |sample_buffer: std::ptr::NonNull<CMSampleBuffer>,
                  kind: RPSampleBufferType,
                  _error: *mut NSError| {
                // SAFETY: `sample_buffer` is valid for the duration of this callback — Apple's
                // documented `startCaptureWithHandler` contract.
                let sample_buffer = unsafe { sample_buffer.as_ref() };
                // SAFETY: same contract.
                unsafe {
                    classify_and_queue_sample_buffer(
                        &queues_cb,
                        &infos_cb,
                        &next_pts_cb,
                        sample_buffer,
                        kind,
                    );
                }
            },
        );

        start_in_app_capture(&recorder, &capture_handler)?;

        Ok(Self {
            inner: Some(InAppSession {
                queues,
                infos,
                recorder,
                _capture_handler: capture_handler,
            }),
        })
    }

    fn close_inner(&mut self) -> Result<(), CaptureError> {
        let Some(session) = self.inner.take() else {
            return Ok(());
        };
        stop_in_app_capture(&session.recorder)
    }
}

impl DesktopVideoCapture for AppleScreenCapture {
    /// Reflects the geometry of the most recently arrived video frame — `unknown_video_info()`
    /// (zeroed placeholder) until the first frame arrives, since `ReplayKit`'s async capture
    /// handler doesn't report format synchronously from `open()` the way most other backends'
    /// setup does.
    fn stream_info(&self) -> &StreamInfo {
        #[allow(
            clippy::option_if_let_else,
            reason = "map_or_else forces 'static vs 'self lifetime clash"
        )]
        if let Some(session) = self.inner.as_ref() {
            session
                .infos
                .video
                .get()
                .unwrap_or_else(|| unknown_video_info())
        } else {
            unknown_video_info()
        }
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, CaptureError> {
        let Some(session) = self.inner.as_ref() else {
            return Err(CaptureError::Closed);
        };
        let mut q = session
            .queues
            .video
            .lock()
            .map_err(|_| CaptureError::Backend)?;
        Ok(q.pop_front())
    }

    fn release_frame(&mut self) -> Result<(), CaptureError> {
        if self.inner.is_none() {
            return Err(CaptureError::Closed);
        }
        Ok(())
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        self.close_inner()
    }
}

impl DesktopAudioCapture for AppleScreenCapture {
    /// Reflects the format of the most recently arrived app-audio frame — see
    /// [`DesktopVideoCapture::stream_info`]'s doc comment for the same "zeroed until first
    /// frame" reasoning.
    fn stream_info(&self) -> &StreamInfo {
        #[allow(
            clippy::option_if_let_else,
            reason = "map_or_else forces 'static vs 'self lifetime clash"
        )]
        if let Some(session) = self.inner.as_ref() {
            session
                .infos
                .app_audio
                .get()
                .unwrap_or_else(|| unknown_audio_info())
        } else {
            unknown_audio_info()
        }
    }

    fn poll_frame(&mut self) -> Result<Option<AudioFrame>, CaptureError> {
        let Some(session) = self.inner.as_ref() else {
            return Err(CaptureError::Closed);
        };
        let mut q = session
            .queues
            .app_audio
            .lock()
            .map_err(|_| CaptureError::Backend)?;
        Ok(q.pop_front())
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        self.close_inner()
    }
}

impl AudioCapture for AppleScreenCapture {
    /// Reflects the format of the most recently arrived microphone-audio frame — see
    /// [`DesktopVideoCapture::stream_info`]'s doc comment for the same "zeroed until first
    /// frame" reasoning.
    fn stream_info(&self) -> &StreamInfo {
        #[allow(
            clippy::option_if_let_else,
            reason = "map_or_else forces 'static vs 'self lifetime clash"
        )]
        if let Some(session) = self.inner.as_ref() {
            session
                .infos
                .mic_audio
                .get()
                .unwrap_or_else(|| unknown_audio_info())
        } else {
            unknown_audio_info()
        }
    }

    fn poll_frame(&mut self) -> Result<Option<AudioFrame>, CaptureError> {
        let Some(session) = self.inner.as_ref() else {
            return Err(CaptureError::Closed);
        };
        let mut q = session
            .queues
            .mic_audio
            .lock()
            .map_err(|_| CaptureError::Backend)?;
        Ok(q.pop_front())
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        self.close_inner()
    }
}

impl Drop for AppleScreenCapture {
    fn drop(&mut self) {
        let _ = self.close_inner();
    }
}

fn start_in_app_capture(
    recorder: &RPScreenRecorder,
    capture_handler: &RcBlock<
        dyn Fn(std::ptr::NonNull<CMSampleBuffer>, RPSampleBufferType, *mut NSError),
    >,
) -> Result<(), CaptureError> {
    let (tx, rx) = sync_channel::<Result<(), CaptureError>>(1);
    let tx = Arc::new(Mutex::new(Some(tx)));
    let completion: RcBlock<dyn Fn(*mut NSError)> = RcBlock::new(move |error: *mut NSError| {
        let result = if error.is_null() {
            Ok(())
        } else {
            Err(CaptureError::Backend)
        };
        if let Ok(mut guard) = tx.lock() {
            if let Some(tx) = guard.take() {
                let _ = tx.send(result);
            }
        }
    });
    // SAFETY: `recorder` is the valid shared singleton; `capture_handler`/`completion` are both
    // valid, kept alive for at least the duration of this call (`capture_handler` for the whole
    // session via the caller's own field, `completion` for this call via this local binding).
    unsafe {
        recorder
            .startCaptureWithHandler_completionHandler(Some(capture_handler), Some(&completion));
    }
    rx.recv_timeout(COMPLETION_TIMEOUT)
        .map_err(|_| CaptureError::Backend)?
}

fn stop_in_app_capture(recorder: &RPScreenRecorder) -> Result<(), CaptureError> {
    let (tx, rx) = sync_channel::<Result<(), CaptureError>>(1);
    let tx = Arc::new(Mutex::new(Some(tx)));
    // `stopCaptureWithHandler`'s completion handler is `Option<&block2::DynBlock<dyn Fn(*mut
    // NSError)>>` per the real generated signature — the same plain (non-`Send`/`Sync`) block
    // shape `startCaptureWithHandler_completionHandler`'s own completion handler uses; this
    // crate's `objc2`/`block2` version has no `Send`/`Sync`-bounded block variant at all.
    let handler: RcBlock<dyn Fn(*mut NSError)> = RcBlock::new(move |error: *mut NSError| {
        let result = if error.is_null() {
            Ok(())
        } else {
            Err(CaptureError::Backend)
        };
        if let Ok(mut guard) = tx.lock() {
            if let Some(tx) = guard.take() {
                let _ = tx.send(result);
            }
        }
    });
    // SAFETY: `recorder` is the valid shared singleton; `handler` is valid for this call.
    unsafe { recorder.stopCaptureWithHandler(Some(&handler)) };
    rx.recv_timeout(COMPLETION_TIMEOUT)
        .map_err(|_| CaptureError::Backend)?
}

// ─────────────────── AppleBroadcastExtensionCapture (Broadcast Upload Extension sink) ───────────────────

struct ExtensionSession {
    queues: Arc<FrameQueues>,
    infos: Arc<StreamInfos>,
    next_pts: AtomicI64,
}

/// Push-in / pull-out sink for a Broadcast Upload Extension's `RPBroadcastSampleHandler`. See
/// module docs and ADR-0004 § Host-extension contract — this type has **no OS session of its
/// own**; the host extension's own real Swift/Objective-C code (this crate cannot write it)
/// calls [`Self::push_sample_buffer`] once per `processSampleBuffer:withType:` invocation.
pub struct AppleBroadcastExtensionCapture {
    inner: Option<ExtensionSession>,
}

impl Default for AppleBroadcastExtensionCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl AppleBroadcastExtensionCapture {
    /// Allocate the shared queues. Does **not** start any OS session — see module docs.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Some(ExtensionSession {
                queues: Arc::new(FrameQueues::new()),
                infos: Arc::new(StreamInfos::new()),
                next_pts: AtomicI64::new(0),
            }),
        }
    }

    /// Hand one sample buffer captured by the host extension's `RPBroadcastSampleHandler` to
    /// this sink. Called from the future `mediaway-ffi` C ABI shim described in ADR-0004 §
    /// FFI boundary — not designed in this crate.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Closed`] if [`Self::close`] was already called.
    pub fn push_sample_buffer(
        &self,
        sample_buffer: &CMSampleBuffer,
        kind: RPSampleBufferType,
    ) -> Result<(), CaptureError> {
        let Some(session) = self.inner.as_ref() else {
            return Err(CaptureError::Closed);
        };
        // SAFETY: caller's contract (this fn's own doc comment) — `sample_buffer` is valid for
        // the duration of the host extension's `processSampleBuffer:withType:` call this
        // forwards.
        unsafe {
            classify_and_queue_sample_buffer(
                &session.queues,
                &session.infos,
                &session.next_pts,
                sample_buffer,
                kind,
            );
        }
        Ok(())
    }

    fn close_inner(&mut self) -> Result<(), CaptureError> {
        // No ReplayKit-related teardown — the host extension's own `broadcastFinished`/
        // `finishBroadcastWithError` handlers own that (ADR-0004 § Host-extension contract
        // step 6). This just drops the queues.
        self.inner = None;
        Ok(())
    }
}

impl DesktopVideoCapture for AppleBroadcastExtensionCapture {
    /// Reflects the geometry of the most recently pushed video frame — zeroed placeholder until
    /// the host extension's first `push_sample_buffer(_, RPSampleBufferType::Video)` call.
    fn stream_info(&self) -> &StreamInfo {
        #[allow(
            clippy::option_if_let_else,
            reason = "map_or_else forces 'static vs 'self lifetime clash"
        )]
        if let Some(session) = self.inner.as_ref() {
            session
                .infos
                .video
                .get()
                .unwrap_or_else(|| unknown_video_info())
        } else {
            unknown_video_info()
        }
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, CaptureError> {
        let Some(session) = self.inner.as_ref() else {
            return Err(CaptureError::Closed);
        };
        let mut q = session
            .queues
            .video
            .lock()
            .map_err(|_| CaptureError::Backend)?;
        Ok(q.pop_front())
    }

    fn release_frame(&mut self) -> Result<(), CaptureError> {
        if self.inner.is_none() {
            return Err(CaptureError::Closed);
        }
        Ok(())
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        self.close_inner()
    }
}

impl DesktopAudioCapture for AppleBroadcastExtensionCapture {
    /// Reflects the format of the most recently pushed app-audio frame — see
    /// [`DesktopVideoCapture::stream_info`]'s doc comment for the same reasoning.
    fn stream_info(&self) -> &StreamInfo {
        #[allow(
            clippy::option_if_let_else,
            reason = "map_or_else forces 'static vs 'self lifetime clash"
        )]
        if let Some(session) = self.inner.as_ref() {
            session
                .infos
                .app_audio
                .get()
                .unwrap_or_else(|| unknown_audio_info())
        } else {
            unknown_audio_info()
        }
    }

    fn poll_frame(&mut self) -> Result<Option<AudioFrame>, CaptureError> {
        let Some(session) = self.inner.as_ref() else {
            return Err(CaptureError::Closed);
        };
        let mut q = session
            .queues
            .app_audio
            .lock()
            .map_err(|_| CaptureError::Backend)?;
        Ok(q.pop_front())
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        self.close_inner()
    }
}

impl AudioCapture for AppleBroadcastExtensionCapture {
    /// Reflects the format of the most recently pushed microphone-audio frame — see
    /// [`DesktopVideoCapture::stream_info`]'s doc comment for the same reasoning.
    fn stream_info(&self) -> &StreamInfo {
        #[allow(
            clippy::option_if_let_else,
            reason = "map_or_else forces 'static vs 'self lifetime clash"
        )]
        if let Some(session) = self.inner.as_ref() {
            session
                .infos
                .mic_audio
                .get()
                .unwrap_or_else(|| unknown_audio_info())
        } else {
            unknown_audio_info()
        }
    }

    fn poll_frame(&mut self) -> Result<Option<AudioFrame>, CaptureError> {
        let Some(session) = self.inner.as_ref() else {
            return Err(CaptureError::Closed);
        };
        let mut q = session
            .queues
            .mic_audio
            .lock()
            .map_err(|_| CaptureError::Backend)?;
        Ok(q.pop_front())
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        self.close_inner()
    }
}

impl Drop for AppleBroadcastExtensionCapture {
    fn drop(&mut self) {
        let _ = self.close_inner();
    }
}

#[cfg(test)]
#[path = "replaykit_tests.rs"]
mod tests;
