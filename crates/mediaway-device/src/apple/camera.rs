//! Apple camera capture via `AVFoundation`'s `AVCaptureSession` + a `define_class!` delegate
//! (`AVCaptureVideoDataOutputSampleBufferDelegate`). See
//! [ADR-0001](adr/apple/0001-avfoundation-camera-capture.md).
//!
//! Unlike `VideoToolbox`'s plain C function-pointer callback (`mediaway-encoder::apple`),
//! `AVCaptureVideoDataOutput`'s frame delivery is a full Objective-C protocol conformance — Rust
//! code defines a real class (`CameraDelegate`, via `objc2`'s `define_class!`) that implements
//! [`AVCaptureVideoDataOutputSampleBufferDelegate`] and hands an instance to
//! `setSampleBufferDelegate:queue:`.
//!
//! # A real correction found while implementing this ADR
//!
//! ADR-0001 § Open questions #6 left `AVCaptureSession::startRunning()`'s call-site
//! (worker-thread vs. calling thread) undecided. Reading the real generated signature —
//! `pub unsafe fn startRunning(&self);`, no error out-parameter, no completion handler — confirms
//! it is a **plain synchronous call**, not the kind of unboundedly-async operation that needs a
//! dedicated bridge thread. `open()` calls it directly on the calling thread; frame delivery
//! still happens asynchronously, but on `libdispatch`'s own internally managed queue thread (this
//! crate creates the [`DispatchQueue`] but does not own or spawn its worker thread itself) — not
//! a pattern this crate needs to bridge with a channel/condvar the way Android's `MediaProjection`
//! or this module's own `screencapturekit`/`replaykit` async-completion-handler backends do.
//!
//! **Zero compile verification** — this dev environment cannot cross-compile Apple code at all
//! outside macOS/Xcode; see the crate's `apple-macos`/`apple-ios` CI jobs.

#![allow(unsafe_code)]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use crate::camera::{CameraCapture, CameraCaptureConfig, CaptureOutputPreference};
use crate::{CaptureError, Select};
use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained};
use mediaway_common::{
    Bytes, CodecKind, PixelFormat, Rational, StreamInfo, VideoFrame, VideoFrameStorage,
    VideoGeometry,
};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_av_foundation::{
    AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceInput, AVCaptureOutput, AVCaptureSession,
    AVCaptureVideoDataOutput, AVCaptureVideoDataOutputSampleBufferDelegate, AVMediaTypeVideo,
};
use objc2_core_media::CMSampleBuffer;
use objc2_core_video::{
    kCVPixelBufferPixelFormatTypeKey, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
};
use objc2_foundation::{NSDictionary, NSNumber, NSObjectProtocol, NSString};

struct FrameQueue {
    frames: Mutex<VecDeque<VideoFrame>>,
}

/// Bounded, drop-oldest delivered-frame queue depth — mirrors `android::camera`'s
/// `FRAME_QUEUE_CAP`.
const FRAME_QUEUE_CAP: usize = 4;

struct CameraDelegateIvars {
    queue: Arc<FrameQueue>,
    next_pts: AtomicI64,
}

define_class!(
    #[unsafe(super(objc2_foundation::NSObject))]
    #[thread_kind = AnyThread]
    #[name = "MediawayCameraDelegate"]
    #[ivars = CameraDelegateIvars]
    struct CameraDelegate;

    unsafe impl NSObjectProtocol for CameraDelegate {}

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for CameraDelegate {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        unsafe fn capture_output_did_output_sample_buffer_from_connection(
            &self,
            _output: &AVCaptureOutput,
            sample_buffer: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
            // SAFETY: `sample_buffer` is valid for the duration of this delegate callback —
            // Apple's documented contract for `captureOutput:didOutputSampleBuffer:fromConnection:`.
            if let Some(data) = unsafe { super::pixel::extract_nv12(sample_buffer) } {
                let pts = self.ivars().next_pts.fetch_add(1, Ordering::Relaxed);
                push_frame(self.ivars().queue.as_ref(), data, pts);
            }
        }
    }
);

impl CameraDelegate {
    fn new(queue: Arc<FrameQueue>) -> Retained<Self> {
        let this = Self::alloc();
        let this = this.set_ivars(CameraDelegateIvars {
            queue,
            next_pts: AtomicI64::new(0),
        });
        // SAFETY: `this` was just allocated and had its ivars set — the standard `define_class!`
        // init pattern (mirrors `objc2`'s own `DropIvars`/`AppDelegate` examples).
        unsafe { msg_send![super(this), init] }
    }
}

struct CameraSession {
    stream_info: StreamInfo,
    queue: Arc<FrameQueue>,
    session: Retained<AVCaptureSession>,
    _input: Retained<AVCaptureDeviceInput>,
    _output: Retained<AVCaptureVideoDataOutput>,
    _delegate: Retained<CameraDelegate>,
    _dispatch_queue: DispatchRetained<DispatchQueue>,
}

/// Apple camera capture session (`AVCaptureSession`, CPU NV12 frames).
///
/// Uses `VideoRange` — the real camera hardware convention, deliberately diverging from
/// `mediaway-encoder::apple`'s `FullRange` choice for its own synthetic encode input. See module
/// docs for scope.
pub struct AppleCameraCapture {
    inner: Option<CameraSession>,
}

impl AppleCameraCapture {
    /// Open `AVCaptureSession` camera capture for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Unsupported`] for a non-[`Select::Default`] selection, the
    /// [`CaptureOutputPreference::ZeroCopyGpu`] preference (not implemented this slice), or when
    /// the device does not report `kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange` as an
    /// available capture format. Returns [`CaptureError::InvalidInput`] when no default video
    /// device exists. Returns [`CaptureError::Backend`] on other `AVFoundation` failures.
    pub fn open(config: &CameraCaptureConfig) -> Result<Self, CaptureError> {
        if config.select != Select::Default {
            return Err(CaptureError::Unsupported);
        }
        if config.output != CaptureOutputPreference::CpuFramesOk {
            return Err(CaptureError::Unsupported);
        }

        // SAFETY: `AVMediaTypeVideo` is a valid, process-lifetime static constant.
        // `AVMediaTypeVideo` itself is `Option<&'static AVMediaType>` (an optional CF/Foundation
        // static), so it must be unwrapped before use as `defaultDeviceWithMediaType`'s plain
        // `&AVMediaType` parameter.
        let media_type = unsafe { AVMediaTypeVideo }.ok_or(CaptureError::Backend)?;
        // SAFETY: plain class method, no preconditions.
        let device = unsafe { AVCaptureDevice::defaultDeviceWithMediaType(media_type) }
            .ok_or(CaptureError::InvalidInput)?;
        // SAFETY: `device` is a valid, just-obtained capture device.
        let input = unsafe { AVCaptureDeviceInput::deviceInputWithDevice_error(&device) }
            .map_err(|_| CaptureError::Backend)?;

        // SAFETY: plain, always-safe-to-call constructor.
        let session = unsafe { AVCaptureSession::new() };
        // SAFETY: `session`/`input` are both valid, freshly created objects.
        if !unsafe { session.canAddInput(&input) } {
            return Err(CaptureError::Backend);
        }
        // SAFETY: same as above.
        unsafe { session.addInput(&input) };

        // SAFETY: plain, always-safe-to-call constructor.
        let output = unsafe { AVCaptureVideoDataOutput::new() };
        // SAFETY: `output` is a valid, freshly created output.
        let available = unsafe { output.availableVideoCVPixelFormatTypes() };
        let requested_format = kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange;
        // `requested_format` is a fixed, known FourCC-style `OSType` constant — reinterpreting
        // its bits as `i32` to compare against `NSNumber::intValue()` never wraps in practice.
        #[allow(
            clippy::cast_possible_wrap,
            reason = "requested_format is a fixed FourCC-style OSType constant, not user input"
        )]
        let requested_format_i32 = requested_format as i32;
        let format_available = available
            .iter()
            .any(|n| n.intValue() == requested_format_i32);
        if !format_available {
            return Err(CaptureError::Unsupported);
        }

        // SAFETY: `kCVPixelBufferPixelFormatTypeKey` is a valid, process-lifetime CFString
        // constant that is toll-free bridged to `NSString` (Apple's documented CoreFoundation/
        // Foundation bridging guarantee) — reinterpreting its reference as `&NSString` is the
        // standard technique for using a `CF*` string constant as an `NSDictionary` key.
        let key: &NSString =
            unsafe { &*std::ptr::from_ref(kCVPixelBufferPixelFormatTypeKey).cast::<NSString>() };
        let value = NSNumber::numberWithUnsignedInt(requested_format);
        // `setVideoSettings` requires `NSDictionary<NSString, AnyObject>` — `NSNumber`'s declared
        // inheritance chain (`NSValue`, `NSObject`) has no direct `AsRef<AnyObject>`, so this goes
        // through a plain `Deref`-coercing reference instead of `.as_ref()` (which would otherwise
        // pin the dictionary's value type to `NSNumber`, not `AnyObject`).
        let value: &AnyObject = &value;
        let settings = NSDictionary::from_slices(&[key], &[value]);
        // SAFETY: `output` is valid; `settings` is a well-formed dictionary with the one
        // documented supported key.
        unsafe { output.setVideoSettings(Some(&settings)) };

        let queue = Arc::new(FrameQueue {
            frames: Mutex::new(VecDeque::new()),
        });
        // clone: delegate ivar needs its own strong ref to push frames
        let delegate = CameraDelegate::new(Arc::clone(&queue));
        // `DispatchQueue::new` takes a plain `&str` label (not `Option<&CStr>`) and is a safe,
        // always-safe-to-call constructor — no `unsafe` needed.
        let dispatch_queue = DispatchQueue::new("dev.mediaway.camera", DispatchQueueAttr::SERIAL);
        let delegate_protocol = ProtocolObject::from_ref(&*delegate);
        // SAFETY: `output` is valid; `delegate_protocol`/`dispatch_queue` are both valid, kept
        // alive by this session's own fields for its whole lifetime.
        unsafe {
            output.setSampleBufferDelegate_queue(Some(delegate_protocol), Some(&dispatch_queue));
        }

        // SAFETY: `session`/`output` are both valid.
        if !unsafe { session.canAddOutput(&output) } {
            return Err(CaptureError::Backend);
        }
        // SAFETY: same as above.
        unsafe { session.addOutput(&output) };

        // SAFETY: `session` is fully configured (one input, one output). Synchronous — see
        // module docs § "A real correction found while implementing this ADR".
        unsafe { session.startRunning() };

        let info = StreamInfo::Video {
            id: 0,
            codec: CodecKind::RawVideo,
            time_base: config.time_base,
            // Real dimensions are only known once the first frame arrives (device-dependent,
            // no fixed resolution requested this slice) — `0x0` here mirrors every other
            // backend's "closed/unknown" placeholder shape; callers read `VideoFrame::width`/
            // `height` off delivered frames.
            geometry: VideoGeometry {
                width: 0,
                height: 0,
            },
            extra_data: Bytes::new(),
        };

        Ok(Self {
            inner: Some(CameraSession {
                stream_info: info,
                queue,
                session,
                _input: input,
                _output: output,
                _delegate: delegate,
                _dispatch_queue: dispatch_queue,
            }),
        })
    }
}

impl CameraCapture for AppleCameraCapture {
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
        // SAFETY: `session.session` is a valid, running `AVCaptureSession`.
        unsafe { session.session.stopRunning() };
        // `session`'s `Drop` (via `Retained`/`DispatchRetained`) releases every Objective-C
        // object it holds.
        Ok(())
    }
}

impl Drop for AppleCameraCapture {
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

fn push_frame(queue: &FrameQueue, data_and_size: (Bytes, u32, u32), pts: i64) {
    let (data, width, height) = data_and_size;
    let frame = VideoFrame {
        pts,
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
#[path = "camera_tests.rs"]
mod tests;
