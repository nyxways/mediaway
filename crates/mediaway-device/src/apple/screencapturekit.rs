//! macOS screen capture via `ScreenCaptureKit`. See
//! [ADR-0003](adr/apple/0003-screencapturekit-macos-screen-capture.md).
//!
//! Genuinely macOS-only — `objc2-screen-capture-kit` does not exist on iOS (see
//! [`super::replaykit`] for the iOS equivalent, `ReplayKit`). This is the first backend in this
//! crate with a genuinely async `open()` sequence: `SCShareableContent`'s enumeration and
//! `SCStream::startCaptureWithCompletionHandler` are both real completion-handler-based async
//! calls (`block2`), bridged to a synchronous return via a bounded channel — not a `block_on`
//! wrapper over a sync-shaped protocol the way Linux portal's D-Bus round trip is.
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
use objc2::{AnyThread, Ivars, define_class, msg_send};
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

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = AnyThread]
    #[name = "MediawayScreenStreamOutput"]
    struct StreamOutput {
        queue: Arc<FrameQueue>,
    }

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
                push_frame(self.queue().as_ref(), data);
            }
        }
    }
);

impl StreamOutput {
    fn new(queue: Arc<FrameQueue>) -> Retained<Self> {
        let this = Self::alloc();
        let this = this.set_ivars(Ivars::<Self> { queue });
        // SAFETY: standard `define_class!` init pattern.
        unsafe { msg_send![super(this), init] }
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = AnyThread]
    #[name = "MediawayScreenStreamDelegate"]
    struct StreamDelegate {
        stopped: Arc<AtomicBool>,
    }

    unsafe impl NSObjectProtocol for StreamDelegate {}

    unsafe impl SCStreamDelegate for StreamDelegate {
        #[unsafe(method(stream:didStopWithError:))]
        unsafe fn stream_did_stop_with_error(&self, _stream: &SCStream, _error: &NSError) {
            self.stopped().store(true, Ordering::SeqCst);
        }
    }
);

impl StreamDelegate {
    fn new(stopped: Arc<AtomicBool>) -> Retained<Self> {
        let this = Self::alloc();
        let this = this.set_ivars(Ivars::<Self> { stopped });
        // SAFETY: standard `define_class!` init pattern.
        unsafe { msg_send![super(this), init] }
    }
}

struct ScreenSession {
    stream_info: StreamInfo,
    queue: Arc<FrameQueue>,
    stopped: Arc<AtomicBool>,
    stream: Retained<SCStream>,
    _output: Retained<StreamOutput>,
    _delegate: Retained<StreamDelegate>,
    _dispatch_queue: DispatchRetained<DispatchQueue>,
}

/// macOS screen capture session (`SCStream`, CPU NV12 frames, primary display only this slice).
/// See module docs for the async `open()` design.
pub struct AppleScreenCapture {
    inner: Option<ScreenSession>,
}

impl AppleScreenCapture {
    /// Open `ScreenCaptureKit` screen capture for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Unsupported`] for a non-[`Select::Default`] selection, a
    /// [`DesktopCaptureSource::Window`] source (not implemented this slice — see ADR-0003 § Open
    /// questions #5), or the [`CaptureOutputPreference::ZeroCopyGpu`] preference. Returns
    /// [`CaptureError::InvalidInput`] when no display is reported. Returns
    /// [`CaptureError::Backend`] on `ScreenCaptureKit` failure or completion-handler timeout.
    pub fn open(config: &DesktopVideoCaptureConfig) -> Result<Self, CaptureError> {
        let DesktopCaptureSource::Screen { select } = config.source else {
            return Err(CaptureError::Unsupported);
        };
        if select != Select::Default {
            return Err(CaptureError::Unsupported);
        }
        if config.output != CaptureOutputPreference::CpuFramesOk {
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
                &filter,
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

        Ok(Self {
            inner: Some(ScreenSession {
                stream_info: info,
                queue,
                stopped,
                stream,
                _output: output,
                _delegate: delegate,
                _dispatch_queue: dispatch_queue,
            }),
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
        let Some(session) = self.inner.as_ref() else {
            return Err(CaptureError::Closed);
        };
        if session.stopped.load(Ordering::Relaxed) {
            return Err(CaptureError::DeviceLost);
        }
        let mut q = session
            .queue
            .frames
            .lock()
            .map_err(|_| CaptureError::Backend)?;
        Ok(q.pop_front())
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
        let _ = stop_capture(&session.stream);
        // `session`'s `Drop` releases every Objective-C object it holds.
        Ok(())
    }
}

impl Drop for AppleScreenCapture {
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

/// Bridges `SCShareableContent::getShareableContentWithCompletionHandler` (a real, confirmed
/// async completion-handler call — see module docs) to a synchronous return.
fn fetch_shareable_content() -> Result<Retained<SCShareableContent>, CaptureError> {
    let (tx, rx) = sync_channel::<Result<Retained<SCShareableContent>, CaptureError>>(1);
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
                // callback returns.
                unsafe { Retained::retain(content) }.ok_or(CaptureError::Backend)
            };
            if let Some(tx) = take_sender(&tx) {
                let _ = tx.send(result);
            }
        },
    );
    // SAFETY: `block` is a valid, kept-alive-for-the-call block reference.
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&block) };
    rx.recv_timeout(COMPLETION_TIMEOUT)
        .map_err(|_| CaptureError::Backend)?
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
