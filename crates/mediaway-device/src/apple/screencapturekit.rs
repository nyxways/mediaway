//! macOS screen **and window** capture via `ScreenCaptureKit`. See
//! [ADR-0003](adr/apple/0003-screencapturekit-macos-screen-capture.md).
//!
//! Genuinely macOS-only — `objc2-screen-capture-kit` does not exist on iOS (see
//! [`super::replaykit`] for the iOS equivalent, `ReplayKit`). This is the first backend in this
//! crate with a genuinely async `open()` sequence: `SCShareableContent`'s enumeration and
//! `SCStream::startCaptureWithCompletionHandler` are both real completion-handler-based async
//! calls (`block2`), bridged to a synchronous return via a bounded channel — not a `block_on`
//! wrapper over a sync-shaped protocol the way Linux portal's D-Bus round trip is.
//!
//! [`AppleScreenCapture`] ([`DesktopCaptureSource::Screen`]) and [`AppleWindowCapture`]
//! ([`DesktopCaptureSource::Window`]) share one `SCStream` session recipe (`open_stream`) — only
//! `SCContentFilter` construction differs (`initWithDisplay_excludingWindows` vs.
//! `initWithDesktopIndependentWindow`, the latter resolving a native-handle window token's bits
//! against `SCShareableContent::windows()`'s `CGWindowID`s) — the same shared-session shape
//! `mediaway-device::linux`'s `LinuxWindowCapture`/`LinuxScreenCapture` use over one portal
//! `Session`, mirrored here rather than re-derived.
//!
//! **Zero compile verification** — this dev environment cannot cross-compile Apple code at all
//! outside macOS/Xcode; see the crate's `apple-macos` CI job.

#![allow(unsafe_code)]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::desktop::{
    CaptureOutputPreference, DesktopCaptureSource, DesktopVideoCapture, DesktopVideoCaptureConfig,
};
use crate::{CaptureError, Select};
use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained};
use mediaway_common::{
    Bytes, CodecKind, PixelFormat, Rational, StreamInfo, VideoFrame, VideoFrameStorage,
    VideoGeometry,
};
use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_core_media::CMSampleBuffer;
use objc2_core_video::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange;
use objc2_foundation::{NSArray, NSError, NSObject};
use objc2_screen_capture_kit::{
    SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamDelegate,
    SCStreamOutput, SCStreamOutputType, SCWindow,
};

/// Fixed capture resolution this slice, matching `android::screencast`'s own first-slice
/// resolution scope limit.
const CAPTURE_WIDTH: usize = 1920;
const CAPTURE_HEIGHT: usize = 1080;

/// How long `open()`/`close()` waits for each `block2` completion handler before giving up — no
/// real hardware this session to tune against (adr/apple/0003 § Open questions #2).
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(10);

struct FrameQueue {
    frames: Mutex<VecDeque<VideoFrame>>,
}

/// Bounded, drop-oldest delivered-frame queue depth — mirrors `apple::camera`/`android::camera`.
const FRAME_QUEUE_CAP: usize = 4;

struct StreamOutputIvars {
    queue: Arc<FrameQueue>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = AnyThread]
    #[name = "MediawayScreenStreamOutput"]
    #[ivars = StreamOutputIvars]
    struct StreamOutput;

    unsafe impl NSObjectProtocol for StreamOutput {}

    unsafe impl SCStreamOutput for StreamOutput {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        unsafe fn stream_did_output_sample_buffer_of_type(
            &self,
            _stream: &SCStream,
            sample_buffer: &CMSampleBuffer,
            of_type: SCStreamOutputType,
        ) {
            if of_type != SCStreamOutputType::Screen {
                return;
            }
            // SAFETY: `sample_buffer` is valid for the duration of this callback — Apple's
            // documented `SCStreamOutput` contract.
            if let Some(data) = unsafe { super::pixel::extract_nv12(sample_buffer) } {
                push_frame(self.ivars().queue.as_ref(), data);
            }
        }
    }
);

impl StreamOutput {
    fn new(queue: Arc<FrameQueue>) -> Retained<Self> {
        let this = Self::alloc();
        let this = this.set_ivars(StreamOutputIvars { queue });
        // SAFETY: standard `define_class!` init pattern.
        unsafe { msg_send![super(this), init] }
    }
}

struct StreamDelegateIvars {
    stopped: Arc<AtomicBool>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = AnyThread]
    #[name = "MediawayScreenStreamDelegate"]
    #[ivars = StreamDelegateIvars]
    struct StreamDelegate;

    unsafe impl NSObjectProtocol for StreamDelegate {}

    unsafe impl SCStreamDelegate for StreamDelegate {
        #[unsafe(method(stream:didStopWithError:))]
        unsafe fn stream_did_stop_with_error(&self, _stream: &SCStream, _error: &NSError) {
            self.ivars().stopped.store(true, Ordering::SeqCst);
        }
    }
);

impl StreamDelegate {
    fn new(stopped: Arc<AtomicBool>) -> Retained<Self> {
        let this = Self::alloc();
        let this = this.set_ivars(StreamDelegateIvars { stopped });
        // SAFETY: standard `define_class!` init pattern.
        unsafe { msg_send![super(this), init] }
    }
}

/// Shared session state for both [`AppleScreenCapture`] and [`AppleWindowCapture`] — the
/// `SCStream`/delegate/dispatch-queue plumbing is identical once a `SCContentFilter` exists; only
/// filter construction differs (see [`open_stream`]).
struct Session {
    stream_info: StreamInfo,
    queue: Arc<FrameQueue>,
    stopped: Arc<AtomicBool>,
    stream: Retained<SCStream>,
    _output: Retained<StreamOutput>,
    _delegate: Retained<StreamDelegate>,
    _dispatch_queue: DispatchRetained<DispatchQueue>,
}

impl Session {
    fn poll_frame(&self) -> Result<Option<VideoFrame>, CaptureError> {
        if self.stopped.load(Ordering::Relaxed) {
            return Err(CaptureError::DeviceLost);
        }
        let mut q = self
            .queue
            .frames
            .lock()
            .map_err(|_| CaptureError::Backend)?;
        Ok(q.pop_front())
    }

    fn close(&self) -> Result<(), CaptureError> {
        stop_capture(&self.stream)
    }
}

/// Builds the `SCStream` session common to screen and window capture from an already-constructed
/// `filter` — stream configuration, frame queue, both delegates, `addStreamOutput`, and
/// `startCaptureWithCompletionHandler`. Callers validate their own [`DesktopCaptureSource`]
/// variant and build `filter` before calling this (this function itself only validates
/// [`CaptureOutputPreference`]).
///
/// # Errors
///
/// Returns [`CaptureError::Unsupported`] for [`CaptureOutputPreference::ZeroCopyGpu`] (this
/// backend is CPU-frame-only this slice). Returns [`CaptureError::Backend`] on `ScreenCaptureKit`
/// failure or completion-handler timeout.
fn open_stream(
    filter: &SCContentFilter,
    config: &DesktopVideoCaptureConfig,
) -> Result<Session, CaptureError> {
    if config.output != CaptureOutputPreference::CpuFramesOk {
        return Err(CaptureError::Unsupported);
    }

    // SAFETY: plain, always-safe-to-call constructor.
    let stream_config = unsafe { SCStreamConfiguration::new() };
    // SAFETY: `stream_config` is a valid, freshly created configuration object; these are
    // plain property setters with no additional preconditions.
    unsafe {
        stream_config.setWidth(CAPTURE_WIDTH);
        stream_config.setHeight(CAPTURE_HEIGHT);
        stream_config.setPixelFormat(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange);
        stream_config.setShowsCursor(true);
    }

    let queue = Arc::new(FrameQueue {
        frames: Mutex::new(VecDeque::new()),
    });
    // clone: output delegate ivar needs its own strong ref to push frames
    let output = StreamOutput::new(Arc::clone(&queue));
    let stopped = Arc::new(AtomicBool::new(false));
    // clone: delegate ivar needs its own strong ref
    let delegate = StreamDelegate::new(Arc::clone(&stopped));

    let stream = SCStream::alloc();
    let delegate_protocol = ProtocolObject::from_ref(&*delegate);
    // SAFETY: `filter`/`stream_config` are both valid, fully configured; `delegate_protocol`
    // is kept alive by this session's own `_delegate` field.
    let stream = unsafe {
        SCStream::initWithFilter_configuration_delegate(
            stream,
            filter,
            &stream_config,
            Some(delegate_protocol),
        )
    };

    // `DispatchQueue::new` takes a plain `&str` label (not `Option<&CStr>`) and is a safe,
    // always-safe-to-call constructor — no `unsafe` needed.
    let dispatch_queue =
        DispatchQueue::new("dev.mediaway.screencapturekit", DispatchQueueAttr::SERIAL);
    let output_protocol = ProtocolObject::from_ref(&*output);
    // SAFETY: `stream` is valid; `output_protocol`/`dispatch_queue` are kept alive by this
    // session's own fields.
    unsafe {
        stream.addStreamOutput_type_sampleHandlerQueue_error(
            output_protocol,
            SCStreamOutputType::Screen,
            Some(&dispatch_queue),
        )
    }
    .map_err(|_| CaptureError::Backend)?;

    start_capture(&stream)?;

    let info = StreamInfo::Video {
        id: 0,
        codec: CodecKind::RawVideo,
        time_base: config.time_base,
        geometry: VideoGeometry {
            width: CAPTURE_WIDTH.try_into().unwrap_or(0),
            height: CAPTURE_HEIGHT.try_into().unwrap_or(0),
        },
        extra_data: Bytes::new(),
    };

    Ok(Session {
        stream_info: info,
        queue,
        stopped,
        stream,
        _output: output,
        _delegate: delegate,
        _dispatch_queue: dispatch_queue,
    })
}

/// macOS screen capture session (`SCStream`, CPU NV12 frames, primary display only this slice).
/// See module docs for the async `open()` design.
pub struct AppleScreenCapture {
    inner: Option<Session>,
}

impl AppleScreenCapture {
    /// Open `ScreenCaptureKit` screen capture for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Unsupported`] for a non-[`DesktopCaptureSource::Screen`] source, a
    /// non-[`Select::Default`] selection, or the [`CaptureOutputPreference::ZeroCopyGpu`]
    /// preference. Returns [`CaptureError::InvalidInput`] when no display is reported. Returns
    /// [`CaptureError::Backend`] on `ScreenCaptureKit` failure or completion-handler timeout.
    pub fn open(config: &DesktopVideoCaptureConfig) -> Result<Self, CaptureError> {
        let DesktopCaptureSource::Screen { select } = &config.source else {
            return Err(CaptureError::Unsupported);
        };
        if *select != Select::Default {
            return Err(CaptureError::Unsupported);
        }

        let content = fetch_shareable_content()?;
        // SAFETY: `content` is a valid, just-fetched `SCShareableContent`.
        let displays = unsafe { content.displays() };
        let display = displays.firstObject().ok_or(CaptureError::InvalidInput)?;
        let excluded = NSArray::<SCWindow>::new();
        let filter = SCContentFilter::alloc();
        // SAFETY: `display` is a valid display from `content`; `excluded` is a valid, empty
        // array.
        let filter = unsafe {
            SCContentFilter::initWithDisplay_excludingWindows(filter, &display, &excluded)
        };

        Ok(Self {
            inner: Some(open_stream(&filter, config)?),
        })
    }
}

impl DesktopVideoCapture for AppleScreenCapture {
    fn stream_info(&self) -> &StreamInfo {
        #[allow(
            clippy::option_if_let_else,
            reason = "map_or_else forces 'static vs 'self lifetime clash"
        )]
        if let Some(s) = self.inner.as_ref() {
            &s.stream_info
        } else {
            closed_video_info()
        }
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, CaptureError> {
        self.inner
            .as_ref()
            .ok_or(CaptureError::Closed)?
            .poll_frame()
    }

    fn release_frame(&mut self) -> Result<(), CaptureError> {
        if self.inner.is_none() {
            return Err(CaptureError::Closed);
        }
        // CPU-owned frames hold no backend resource to release.
        Ok(())
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        let Some(session) = self.inner.take() else {
            return Ok(());
        };
        let _ = session.close();
        // `session`'s `Drop` releases every Objective-C object it holds.
        Ok(())
    }
}

impl Drop for AppleScreenCapture {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// macOS window capture session — the same `SCStream` recipe as [`AppleScreenCapture`], filtered
/// to one `SCWindow` (`SCContentFilter::initWithDesktopIndependentWindow`) instead of a display.
/// See [ADR-0003](adr/apple/0003-screencapturekit-macos-screen-capture.md) § Open questions #5.
pub struct AppleWindowCapture {
    inner: Option<Session>,
}

impl AppleWindowCapture {
    /// Open `ScreenCaptureKit` window capture for `config`.
    ///
    /// The [`DesktopCaptureSource::Window`] handle's bits must equal a `CGWindowID` currently
    /// reported by `SCShareableContent::windows()` — unlike the Linux portal backend (whose
    /// picker UI chooses the window interactively, ignoring the handle), `ScreenCaptureKit` can
    /// target a specific window programmatically, the same capability `WGC`'s
    /// `CreateForWindow(HWND)` gives Windows.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Unsupported`] for a non-[`DesktopCaptureSource::Window`] source or
    /// the [`CaptureOutputPreference::ZeroCopyGpu`] preference. Returns
    /// [`CaptureError::InvalidInput`] when the handle's bits do not fit a `u32` `CGWindowID`, or
    /// match none of `SCShareableContent`'s currently reported windows. Returns
    /// [`CaptureError::Backend`] on `ScreenCaptureKit` failure or completion-handler timeout.
    pub fn open(config: &DesktopVideoCaptureConfig) -> Result<Self, CaptureError> {
        let DesktopCaptureSource::Window { window } = &config.source else {
            return Err(CaptureError::Unsupported);
        };
        let window_id = u32::try_from(window.get()).map_err(|_| CaptureError::InvalidInput)?;

        let content = fetch_shareable_content()?;
        // SAFETY: `content` is a valid, just-fetched `SCShareableContent`.
        let windows = unsafe { content.windows() };
        let target = windows
            .iter()
            .find(|w| {
                // SAFETY: `w` is a valid `SCWindow` yielded from `content.windows()`.
                unsafe { w.windowID() == window_id }
            })
            .ok_or(CaptureError::InvalidInput)?;
        let filter = SCContentFilter::alloc();
        // SAFETY: `target` is a valid `SCWindow` found in `content.windows()`.
        let filter = unsafe { SCContentFilter::initWithDesktopIndependentWindow(filter, &target) };

        Ok(Self {
            inner: Some(open_stream(&filter, config)?),
        })
    }
}

impl DesktopVideoCapture for AppleWindowCapture {
    fn stream_info(&self) -> &StreamInfo {
        #[allow(
            clippy::option_if_let_else,
            reason = "map_or_else forces 'static vs 'self lifetime clash"
        )]
        if let Some(s) = self.inner.as_ref() {
            &s.stream_info
        } else {
            closed_video_info()
        }
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, CaptureError> {
        self.inner
            .as_ref()
            .ok_or(CaptureError::Closed)?
            .poll_frame()
    }

    fn release_frame(&mut self) -> Result<(), CaptureError> {
        if self.inner.is_none() {
            return Err(CaptureError::Closed);
        }
        // CPU-owned frames hold no backend resource to release.
        Ok(())
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        let Some(session) = self.inner.take() else {
            return Ok(());
        };
        let _ = session.close();
        // `session`'s `Drop` releases every Objective-C object it holds.
        Ok(())
    }
}

impl Drop for AppleWindowCapture {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn closed_video_info() -> &'static StreamInfo {
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

/// Takes the sender out of `state` if it hasn't already been taken — every completion-handler
/// bridge in this module uses this so a block that (against documented contract) fires more
/// than once never panics on a stale `SyncSender`.
fn take_sender<T>(state: &Mutex<Option<SyncSender<T>>>) -> Option<SyncSender<T>> {
    state.lock().ok().and_then(|mut guard| guard.take())
}

/// Carries a `Retained::into_raw` pointer across the `sync_channel` in
/// [`fetch_shareable_content`]. Plain `*mut T` is `!Send` (Rust never auto-derives `Send` for raw
/// pointers, unlike references) even though this specific value is safe to hand to another
/// thread: it is a `+1`-retained, uniquely-owned handle that no other thread touches until
/// [`Retained::from_raw`] reconstitutes it — Apple's retain/release calls themselves are safe
/// from any thread, so only the Rust-level `Send` bound needed asserting.
struct SendRetainedPtr<T>(*mut T);

// SAFETY: see the struct's own doc comment — the pointer is a uniquely-owned, `+1`-retained
// handle at the point it is sent, never concurrently accessed from two threads.
unsafe impl<T> Send for SendRetainedPtr<T> {}

/// Bridges `SCShareableContent::getShareableContentWithCompletionHandler` (a real, confirmed
/// async completion-handler call — see module docs) to a synchronous return.
///
/// The completion handler may run on any thread the OS chooses, so the retained result crosses
/// threads via this function's `sync_channel`. `Retained<SCShareableContent>` is not `Send`
/// (`SCShareableContent`'s thread-safety is undocumented), so the channel carries a
/// [`SendRetainedPtr`]-wrapped `+1` pointer instead of the `Retained` itself; only the actual
/// retain/release calls (safe on any thread per Apple's memory-management contract) happen on
/// either side of the crossing.
fn fetch_shareable_content() -> Result<Retained<SCShareableContent>, CaptureError> {
    let (tx, rx) = sync_channel::<Result<SendRetainedPtr<SCShareableContent>, CaptureError>>(1);
    let tx = Arc::new(Mutex::new(Some(tx)));
    // `RcBlock<F>` takes exactly one generic parameter — the `dyn Fn(...)` block signature itself
    // — not a separate lifetime/fn-pointer/marker-trait triple.
    let block: RcBlock<dyn Fn(*mut SCShareableContent, *mut NSError)> = RcBlock::new(
        move |content: *mut SCShareableContent, _error: *mut NSError| {
            let result = if content.is_null() {
                Err(CaptureError::Backend)
            } else {
                // SAFETY: a completion-handler callback parameter is a borrowed (+0) object per
                // ordinary Cocoa convention; `retain` takes ownership for use after this
                // callback returns; `into_raw` hands that +1 ownership across the channel as a
                // plain pointer value.
                unsafe { Retained::retain(content) }
                    .map(|r| SendRetainedPtr(Retained::into_raw(r)))
                    .ok_or(CaptureError::Backend)
            };
            if let Some(tx) = take_sender(&tx) {
                let _ = tx.send(result);
            }
        },
    );
    // SAFETY: `block` is a valid, kept-alive-for-the-call block reference.
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&block) };
    let ptr = rx
        .recv_timeout(COMPLETION_TIMEOUT)
        .map_err(|_| CaptureError::Backend)??;
    // SAFETY: `ptr.0` is exactly the `Retained::into_raw` pointer produced above, carrying the
    // same +1 retain count `from_raw` expects.
    unsafe { Retained::from_raw(ptr.0) }.ok_or(CaptureError::Backend)
}

fn start_capture(stream: &SCStream) -> Result<(), CaptureError> {
    let (tx, rx) = sync_channel::<Result<(), CaptureError>>(1);
    let tx = Arc::new(Mutex::new(Some(tx)));
    let block: RcBlock<dyn Fn(*mut NSError)> = RcBlock::new(move |error: *mut NSError| {
        let result = if error.is_null() {
            Ok(())
        } else {
            Err(CaptureError::Backend)
        };
        if let Some(tx) = take_sender(&tx) {
            let _ = tx.send(result);
        }
    });
    // SAFETY: `stream` is a valid, fully configured stream; `block` is a valid, kept-alive-for-
    // the-call block reference.
    unsafe { stream.startCaptureWithCompletionHandler(Some(&block)) };
    rx.recv_timeout(COMPLETION_TIMEOUT)
        .map_err(|_| CaptureError::Backend)?
}

fn stop_capture(stream: &SCStream) -> Result<(), CaptureError> {
    let (tx, rx) = sync_channel::<Result<(), CaptureError>>(1);
    let tx = Arc::new(Mutex::new(Some(tx)));
    let block: RcBlock<dyn Fn(*mut NSError)> = RcBlock::new(move |error: *mut NSError| {
        let result = if error.is_null() {
            Ok(())
        } else {
            Err(CaptureError::Backend)
        };
        if let Some(tx) = take_sender(&tx) {
            let _ = tx.send(result);
        }
    });
    // SAFETY: `stream` is a valid, running stream; `block` is a valid, kept-alive-for-the-call
    // block reference.
    unsafe { stream.stopCaptureWithCompletionHandler(Some(&block)) };
    rx.recv_timeout(COMPLETION_TIMEOUT)
        .map_err(|_| CaptureError::Backend)?
}

fn push_frame(queue: &FrameQueue, data_and_size: (Bytes, u32, u32)) {
    let (data, width, height) = data_and_size;
    let frame = VideoFrame {
        pts: 0,
        duration: 1,
        width,
        height,
        format: PixelFormat::Nv12,
        storage: VideoFrameStorage::Cpu { data },
    };
    if let Ok(mut q) = queue.frames.lock() {
        if q.len() >= FRAME_QUEUE_CAP {
            let _ = q.pop_front();
        }
        q.push_back(frame);
    }
}

#[cfg(test)]
#[path = "screencapturekit_tests.rs"]
mod tests;
