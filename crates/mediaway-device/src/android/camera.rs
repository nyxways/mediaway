//! Android camera capture via the Camera2 NDK (raw `ndk-sys` FFI — no safe wrapper crate exists
//! for this API, unlike `ndk::audio`/`ndk::media`). See
//! [ADR-0001](adr/android/0001-camera2-ndk-native-camera-capture.md).
//!
//! # Correction found while implementing this ADR (not just researching it)
//!
//! ADR-0001's design assumed [`ACameraManager_openCamera`] was purely asynchronous, gated on an
//! `onOpened` state callback. Reading the real `ndk-sys` FFI (this module's own source of
//! truth, not memory) shows [`ACameraDevice_StateCallbacks`] has only `onDisconnected`/`onError`
//! fields — **no `onOpened` member exists**. `ACameraManager_openCamera` is a synchronous call:
//! its `camera_status_t` return value and `*device` out-parameter *are* the open outcome; the
//! state callbacks are for *post-open* disconnect/error notification on an already-open device,
//! not an open-completion signal. This module therefore opens synchronously on the worker
//! thread — no channel/condvar bridge needed, simpler than ADR-0001 § Decision originally
//! described.
//!
//! # Scope (this slice)
//!
//! - `Select::Default` only (first camera reporting `BACKWARD_COMPATIBLE` capability is *not*
//!   filtered for — the first entry in `ACameraManager_getCameraIdList`'s ordinal order is used
//!   as-is, mirroring `linux::camera`'s own "no semantic filtering" first cut).
//! - One fixed capture resolution ([`CAPTURE_WIDTH`]×[`CAPTURE_HEIGHT`]) — no
//!   `StreamConfigurationMap`/`ACameraMetadata` querying.
//! - `YUV_420_888` plane layout: only the fully-planar case (I420 byte order) and the
//!   semi-planar, pointer-adjacent-U-then-V case (NV12) are accepted — see
//!   [`detect_pixel_format`]. A device reporting V-before-U (NV21-shaped) has no supported
//!   format this slice (`PixelFormat::Nv21` does not exist in `mediaway-common` yet).
//! - CPU frames only ([`CaptureOutputPreference::CpuFramesOk`]) — no `HardwareBuffer` Zero-Copy
//!   path (`Image::hardware_buffer()`, `GpuBufferHandle::AndroidSurface`) this slice.
//! - Frames are pulled by polling [`ImageReader::acquire_latest_image`] on a fixed interval,
//!   not via `AImageReader_setImageListener`'s async callback — avoids a second FFI callback
//!   layer for a first slice with zero real-hardware verification either way.
//!
//! **Zero compile verification, zero real-hardware verification** — see the crate's `android`
//! CI job.

#![allow(unsafe_code)]

use std::collections::VecDeque;
use std::ffi::{CStr, c_void};
use std::os::raw::c_int;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::camera::{CameraCapture, CameraCaptureConfig, CaptureOutputPreference};
use crate::{CaptureError, Select};
use mediaway_common::{
    Bytes, CodecKind, PixelFormat, Rational, StreamInfo, VideoFrame, VideoFrameStorage,
    VideoGeometry,
};
use ndk::media::image_reader::{AcquireResult, Image, ImageFormat, ImageReader};

/// Fixed capture resolution this slice — see module docs § Scope.
const CAPTURE_WIDTH: i32 = 1280;
const CAPTURE_HEIGHT: i32 = 720;

/// `AImageReader` internal buffer count, independent of the delivered-frame queue below.
const MAX_IMAGES: i32 = 4;

/// Bounded, drop-oldest delivered-frame queue depth — mirrors `linux::camera`'s
/// `FRAME_QUEUE_CAP`.
const FRAME_QUEUE_CAP: usize = 4;

/// How often the worker polls `acquire_latest_image` and rechecks the stop flag.
const POLL_INTERVAL: Duration = Duration::from_millis(8);

struct FrameQueue {
    frames: Mutex<VecDeque<VideoFrame>>,
}

struct CameraSession {
    stream_info: StreamInfo,
    queue: Arc<FrameQueue>,
    stop: Arc<AtomicBool>,
    /// Set by [`on_device_disconnected`]/[`on_device_error`] — checked by `poll_frame` in
    /// addition to popping the queue, so a lost device is reported even once the queue drains.
    device_lost: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

/// Android camera capture session (Camera2 NDK, CPU frames). See module docs for format/
/// resolution scope.
pub struct AndroidCameraCapture {
    inner: Option<CameraSession>,
}

impl AndroidCameraCapture {
    /// Open Camera2 NDK capture for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Unsupported`] for a non-[`Select::Default`] selection or the
    /// [`CaptureOutputPreference::ZeroCopyGpu`] preference (not implemented — see module docs),
    /// or when the opened device's negotiated `YUV_420_888` plane layout doesn't provably match
    /// [`PixelFormat::I420`]/[`PixelFormat::Nv12`]. Returns [`CaptureError::InvalidInput`] when
    /// no camera exists at ordinal `0`. Returns [`CaptureError::Backend`] on other Camera2/NDK
    /// failures.
    pub fn open(config: &CameraCaptureConfig) -> Result<Self, CaptureError> {
        if config.select != Select::Default {
            return Err(CaptureError::Unsupported);
        }
        if config.output != CaptureOutputPreference::CpuFramesOk {
            return Err(CaptureError::Unsupported);
        }

        let queue = Arc::new(FrameQueue {
            frames: Mutex::new(VecDeque::new()),
        });
        // clone: Arc share with camera worker thread
        let queue_worker = Arc::clone(&queue);
        let stop = Arc::new(AtomicBool::new(false));
        // clone: Arc share with camera worker thread
        let stop_worker = Arc::clone(&stop);
        let device_lost = Arc::new(AtomicBool::new(false));
        // clone: Arc share with camera worker thread (written from C state callbacks)
        let device_lost_worker = Arc::clone(&device_lost);
        let time_base = config.time_base;

        let (tx_info, rx_info) = std::sync::mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("mediaway-camera2-ndk".into())
            .spawn(move || {
                run_camera_worker(
                    time_base,
                    &queue_worker,
                    &stop_worker,
                    &device_lost_worker,
                    &tx_info,
                );
            })
            .map_err(|_| CaptureError::Backend)?;

        let stream_info = rx_info.recv().map_err(|_| CaptureError::Backend)??;

        Ok(Self {
            inner: Some(CameraSession {
                stream_info,
                queue,
                stop,
                device_lost,
                worker: Some(worker),
            }),
        })
    }
}

impl CameraCapture for AndroidCameraCapture {
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
        if session.device_lost.load(Ordering::Relaxed) {
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

    /// Signals the worker's stop flag and joins it. The worker rechecks the stop flag every
    /// [`POLL_INTERVAL`] between image polls.
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

impl Drop for AndroidCameraCapture {
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

/// Context boxed for the `ACameraDevice_StateCallbacks` — kept alive for the whole session so
/// the raw `context` pointer handed to the NDK stays valid.
struct DeviceState {
    lost: Arc<AtomicBool>,
}

unsafe extern "C" fn on_device_disconnected(
    context: *mut c_void,
    _device: *mut ndk_sys::ACameraDevice,
) {
    // SAFETY: `context` was set from a `Box<DeviceState>` kept alive by `CameraResources` for
    // the whole session this callback can fire during.
    if let Some(state) = unsafe { (context.cast::<DeviceState>()).as_ref() } {
        state.lost.store(true, Ordering::SeqCst);
    }
}

unsafe extern "C" fn on_device_error(
    context: *mut c_void,
    _device: *mut ndk_sys::ACameraDevice,
    _error: c_int,
) {
    // SAFETY: same as `on_device_disconnected`.
    if let Some(state) = unsafe { (context.cast::<DeviceState>()).as_ref() } {
        state.lost.store(true, Ordering::SeqCst);
    }
}

/// Owns the whole raw Camera2 NDK FFI resource chain for one session. `Drop` tears down
/// whatever is non-null, in reverse acquisition order — every field starts null/`None` and is
/// only set once its own acquisition step succeeds, so an early-return error path during
/// `open_camera_session` never leaks a partially built chain.
struct CameraResources {
    manager: *mut ndk_sys::ACameraManager,
    device: *mut ndk_sys::ACameraDevice,
    session: *mut ndk_sys::ACameraCaptureSession,
    request: *mut ndk_sys::ACaptureRequest,
    output_target: *mut ndk_sys::ACameraOutputTarget,
    output_container: *mut ndk_sys::ACaptureSessionOutputContainer,
    session_output: *mut ndk_sys::ACaptureSessionOutput,
    /// Keeps the `ImageReader` (and the `NativeWindow` the capture session's output target
    /// points at) alive for the whole session.
    image_reader: Option<ImageReader>,
    /// Keeps the boxed [`DeviceState`] alive for the whole session — dropped last, after
    /// `device`/`session` are closed so no callback can fire on a freed context.
    device_state: Option<Box<DeviceState>>,
}

impl Default for CameraResources {
    fn default() -> Self {
        Self {
            manager: std::ptr::null_mut(),
            device: std::ptr::null_mut(),
            session: std::ptr::null_mut(),
            request: std::ptr::null_mut(),
            output_target: std::ptr::null_mut(),
            output_container: std::ptr::null_mut(),
            session_output: std::ptr::null_mut(),
            image_reader: None,
            device_state: None,
        }
    }
}

impl Drop for CameraResources {
    fn drop(&mut self) {
        // SAFETY: each `_free`/`_close`/`_delete` call below is only reached for a pointer this
        // struct itself set to non-null after its matching `_create`/`_open` call returned
        // success — never a dangling or already-freed pointer.
        unsafe {
            if !self.request.is_null() {
                ndk_sys::ACaptureRequest_free(self.request);
            }
            if !self.output_target.is_null() {
                ndk_sys::ACameraOutputTarget_free(self.output_target);
            }
            if !self.session.is_null() {
                ndk_sys::ACameraCaptureSession_close(self.session);
            }
            if !self.output_container.is_null() && !self.session_output.is_null() {
                ndk_sys::ACaptureSessionOutputContainer_remove(
                    self.output_container,
                    self.session_output,
                );
            }
            if !self.session_output.is_null() {
                ndk_sys::ACaptureSessionOutput_free(self.session_output);
            }
            if !self.output_container.is_null() {
                ndk_sys::ACaptureSessionOutputContainer_free(self.output_container);
            }
            if !self.device.is_null() {
                ndk_sys::ACameraDevice_close(self.device);
            }
            if !self.manager.is_null() {
                ndk_sys::ACameraManager_delete(self.manager);
            }
        }
        // `image_reader`/`device_state` drop after this fn body via their own `Drop` impls
        // (struct field order — declared after the raw-pointer fields above).
    }
}

fn run_camera_worker(
    time_base: Rational,
    queue: &FrameQueue,
    stop: &AtomicBool,
    device_lost: &Arc<AtomicBool>,
    tx_info: &std::sync::mpsc::SyncSender<Result<StreamInfo, CaptureError>>,
) {
    let (resources, stream_info, format) =
        match open_camera_session(time_base, Arc::clone(device_lost)) {
            Ok(v) => v,
            Err(e) => {
                let _ = tx_info.send(Err(e));
                return;
            }
        };
    let _ = tx_info.send(Ok(stream_info));

    let Some(reader) = resources.image_reader.as_ref() else {
        return;
    };
    let mut pts: i64 = 0;
    while !stop.load(Ordering::Relaxed) {
        match reader.acquire_latest_image() {
            Ok(AcquireResult::Image(image)) => {
                if let Some(data) = pack_image(&image, format) {
                    push_frame(queue, format, data, pts);
                    pts = pts.saturating_add(1);
                }
                // `image`'s `Drop` (`AImage_delete`) releases the buffer back to the reader.
            }
            Ok(_) | Err(_) => thread::sleep(POLL_INTERVAL),
        }
    }
    // `resources`'s `Drop` tears down the whole Camera2 NDK chain.
}

fn open_camera_session(
    time_base: Rational,
    device_lost: Arc<AtomicBool>,
) -> Result<(CameraResources, StreamInfo, PixelFormat), CaptureError> {
    let mut res = CameraResources::default();
    open_camera_device(&mut res, device_lost)?;
    let format = configure_capture_session(&mut res)?;

    let info = StreamInfo::Video {
        id: 0,
        codec: CodecKind::RawVideo,
        time_base,
        geometry: VideoGeometry {
            width: CAPTURE_WIDTH.unsigned_abs(),
            height: CAPTURE_HEIGHT.unsigned_abs(),
        },
        extra_data: Bytes::new(),
    };
    Ok((res, info, format))
}

/// Creates the `ACameraManager`, resolves the first camera ID, and opens the device
/// synchronously (see module docs § Correction) — the first half of the former
/// `open_camera_session` body, split out to stay under `clippy::too_many_lines`.
fn open_camera_device(
    res: &mut CameraResources,
    device_lost: Arc<AtomicBool>,
) -> Result<(), CaptureError> {
    // SAFETY: `ACameraManager_create` either returns a valid, owned pointer or null (checked
    // below) — no preconditions.
    res.manager = unsafe { ndk_sys::ACameraManager_create() };
    if res.manager.is_null() {
        return Err(CaptureError::Backend);
    }

    let camera_id = first_camera_id(res.manager)?;

    let device_state = Box::new(DeviceState { lost: device_lost });
    let device_state_ptr: *mut DeviceState = (&raw const *device_state).cast_mut();
    let mut state_callbacks = ndk_sys::ACameraDevice_StateCallbacks {
        context: device_state_ptr.cast(),
        onDisconnected: Some(on_device_disconnected),
        onError: Some(on_device_error),
    };
    res.device_state = Some(device_state);

    // SAFETY: `res.manager` is a valid, just-created manager; `camera_id` is a NUL-terminated
    // C string from `ACameraIdList` (freed only after this call, in `first_camera_id`);
    // `state_callbacks` outlives the device via `res.device_state`.
    let status = unsafe {
        ndk_sys::ACameraManager_openCamera(
            res.manager,
            camera_id.as_ptr(),
            &raw mut state_callbacks,
            &raw mut res.device,
        )
    };
    if status.0 != 0 || res.device.is_null() {
        return Err(map_camera_status(status));
    }
    Ok(())
}

/// Sets up the `ImageReader`, capture session, and repeating request on `res.device` (already
/// open), then detects the real negotiated plane format from the first acquired image — the
/// second half of the former `open_camera_session` body, split out to stay under
/// `clippy::too_many_lines`.
fn configure_capture_session(res: &mut CameraResources) -> Result<PixelFormat, CaptureError> {
    let reader = ImageReader::new(
        CAPTURE_WIDTH,
        CAPTURE_HEIGHT,
        ImageFormat::YUV_420_888,
        MAX_IMAGES,
    )
    .map_err(|_| CaptureError::Backend)?;
    let window = reader.window().map_err(|_| CaptureError::Backend)?;
    let anw: *mut ndk_sys::ANativeWindow = window.ptr().as_ptr();
    // `window`'s `NativeWindow` handle is dropped here; the underlying `ANativeWindow` stays
    // alive because `reader` (stored below) still owns a reference to it.
    drop(window);
    res.image_reader = Some(reader);

    // SAFETY: `res.device` is the just-opened device; `anw` is the reader's own window,
    // outliving this call via `res.image_reader`.
    let status =
        unsafe { ndk_sys::ACaptureSessionOutputContainer_create(&raw mut res.output_container) };
    if status.0 != 0 {
        return Err(map_camera_status(status));
    }
    // SAFETY: `anw` is a valid `ACameraWindowType` (`ANativeWindow` alias) for the session's
    // whole lifetime, held alive by `res.image_reader`.
    let status =
        unsafe { ndk_sys::ACaptureSessionOutput_create(anw.cast(), &raw mut res.session_output) };
    if status.0 != 0 {
        return Err(map_camera_status(status));
    }
    // SAFETY: both pointers are non-null, just created above.
    let status = unsafe {
        ndk_sys::ACaptureSessionOutputContainer_add(res.output_container, res.session_output)
    };
    if status.0 != 0 {
        return Err(map_camera_status(status));
    }

    let session_callbacks = ndk_sys::ACameraCaptureSession_stateCallbacks {
        context: std::ptr::null_mut(),
        onClosed: None,
        onReady: None,
        onActive: None,
    };
    // SAFETY: `res.device` is open; `res.output_container` holds the one configured output.
    let status = unsafe {
        ndk_sys::ACameraDevice_createCaptureSession(
            res.device,
            res.output_container,
            &raw const session_callbacks,
            &raw mut res.session,
        )
    };
    if status.0 != 0 {
        return Err(map_camera_status(status));
    }

    // SAFETY: `res.device` is open.
    let status = unsafe {
        ndk_sys::ACameraDevice_createCaptureRequest(
            res.device,
            ndk_sys::ACameraDevice_request_template::TEMPLATE_PREVIEW,
            &raw mut res.request,
        )
    };
    if status.0 != 0 {
        return Err(map_camera_status(status));
    }
    // SAFETY: `anw` outlives this call via `res.image_reader`.
    let status =
        unsafe { ndk_sys::ACameraOutputTarget_create(anw.cast(), &raw mut res.output_target) };
    if status.0 != 0 {
        return Err(map_camera_status(status));
    }
    // SAFETY: `res.request`/`res.output_target` are both non-null, just created above.
    let status = unsafe { ndk_sys::ACaptureRequest_addTarget(res.request, res.output_target) };
    if status.0 != 0 {
        return Err(map_camera_status(status));
    }

    let mut sequence_id: c_int = 0;
    let mut requests = [res.request];
    // SAFETY: `res.session` is open; `requests` holds one valid, fully configured request.
    let status = unsafe {
        ndk_sys::ACameraCaptureSession_setRepeatingRequest(
            res.session,
            std::ptr::null_mut(),
            1,
            requests.as_mut_ptr(),
            &raw mut sequence_id,
        )
    };
    if status.0 != 0 {
        return Err(map_camera_status(status));
    }

    // First image acquired below determines the real plane layout — `open()` fails
    // (`CaptureError::Unsupported`) rather than guessing when it doesn't provably match.
    detect_first_image_format(res.image_reader.as_ref().ok_or(CaptureError::Backend)?)
}

/// Blocks briefly (bounded retries at [`POLL_INTERVAL`]) for the first repeating-request image
/// to determine the real negotiated plane layout, per module docs § Scope.
fn detect_first_image_format(reader: &ImageReader) -> Result<PixelFormat, CaptureError> {
    const MAX_ATTEMPTS: u32 = 250; // ~2s at POLL_INTERVAL — first-frame latency budget
    for _ in 0..MAX_ATTEMPTS {
        if let Ok(AcquireResult::Image(image)) = reader.acquire_latest_image() {
            return detect_pixel_format(&image).ok_or(CaptureError::Unsupported);
        }
        thread::sleep(POLL_INTERVAL);
    }
    Err(CaptureError::Backend)
}

/// Real `ACameraManager_getCameraIdList` count, no device opened — used by
/// `capabilities::camera_support` as a cheaper-than-`open` support probe, mirroring
/// `linux::camera::enumerate_camera_paths`'s own cost class.
pub(super) fn camera_id_count() -> usize {
    // SAFETY: `ACameraManager_create` either returns a valid, owned pointer or null (checked
    // below) — no preconditions.
    let manager = unsafe { ndk_sys::ACameraManager_create() };
    if manager.is_null() {
        return 0;
    }
    let mut list: *mut ndk_sys::ACameraIdList = std::ptr::null_mut();
    // SAFETY: `manager` is valid and non-null (checked above).
    let status = unsafe { ndk_sys::ACameraManager_getCameraIdList(manager, &raw mut list) };
    let count = if status.0 == 0 && !list.is_null() {
        // SAFETY: `list` is non-null (checked above) and was just populated by the call above.
        let n = unsafe { (*list).numCameras };
        usize::try_from(n).unwrap_or(0)
    } else {
        0
    };
    if !list.is_null() {
        // SAFETY: `list` is non-null and was allocated by the matching `getCameraIdList` call.
        unsafe { ndk_sys::ACameraManager_deleteCameraIdList(list) };
    }
    // SAFETY: `manager` is non-null and owned by this function.
    unsafe { ndk_sys::ACameraManager_delete(manager) };
    count
}

fn first_camera_id(
    manager: *mut ndk_sys::ACameraManager,
) -> Result<std::ffi::CString, CaptureError> {
    let mut list: *mut ndk_sys::ACameraIdList = std::ptr::null_mut();
    // SAFETY: `manager` is a valid, non-null, just-created manager.
    let status = unsafe { ndk_sys::ACameraManager_getCameraIdList(manager, &raw mut list) };
    if status.0 != 0 || list.is_null() {
        return Err(map_camera_status(status));
    }
    // SAFETY: `list` is non-null (checked above); `ACameraManager_getCameraIdList` guarantees
    // `numCameras` matches the real `cameraIds` array length.
    let result = unsafe {
        let list_ref = &*list;
        if list_ref.numCameras <= 0 || list_ref.cameraIds.is_null() {
            Err(CaptureError::InvalidInput)
        } else {
            let first = *list_ref.cameraIds;
            if first.is_null() {
                Err(CaptureError::Backend)
            } else {
                Ok(CStr::from_ptr(first).to_owned())
            }
        }
    };
    // SAFETY: `list` is non-null and was allocated by the matching `getCameraIdList` call above.
    unsafe { ndk_sys::ACameraManager_deleteCameraIdList(list) };
    result
}

const fn map_camera_status(status: ndk_sys::camera_status_t) -> CaptureError {
    match status.0 {
        -10013 => CaptureError::AccessDenied, // ACAMERA_ERROR_PERMISSION_DENIED
        -10002 => CaptureError::DeviceLost,   // ACAMERA_ERROR_CAMERA_DISCONNECTED
        -10001 => CaptureError::InvalidInput, // ACAMERA_ERROR_INVALID_PARAMETER
        _ => CaptureError::Backend,
    }
}

/// Determine whether `image`'s `YUV_420_888` planes provably match [`PixelFormat::I420`]
/// (fully planar) or [`PixelFormat::Nv12`] (semi-planar, U immediately before V in one shared
/// buffer) — `None` for anything else, including the semi-planar V-before-U (NV21-shaped) case,
/// which has no supported [`PixelFormat`] variant yet. See module docs § Scope.
fn detect_pixel_format(image: &Image) -> Option<PixelFormat> {
    let u_stride = image.plane_pixel_stride(1).ok()?;
    let v_stride = image.plane_pixel_stride(2).ok()?;
    if u_stride == 1 && v_stride == 1 {
        return Some(PixelFormat::I420);
    }
    if u_stride == 2 && v_stride == 2 {
        let u = image.plane_data(1).ok()?;
        let v = image.plane_data(2).ok()?;
        if !u.is_empty() && v.as_ptr() as usize == (u.as_ptr() as usize).wrapping_add(1) {
            return Some(PixelFormat::Nv12);
        }
    }
    None
}

/// Copy `rows` rows of `row_bytes` bytes each out of `src` at `stride` spacing, appending them
/// tightly packed (no padding) to `out`. Mirrors `linux::camera`'s `copy_rows`.
fn copy_rows(
    src: &[u8],
    out: &mut Vec<u8>,
    row_bytes: usize,
    rows: usize,
    stride: usize,
) -> Option<()> {
    for row in 0..rows {
        let start = row.checked_mul(stride)?;
        let end = start.checked_add(row_bytes)?;
        out.extend_from_slice(src.get(start..end)?);
    }
    Some(())
}

/// Build one tightly packed frame buffer from `image`'s planes, honoring each plane's real
/// `plane_row_stride` (which may exceed the tight row width — device alignment padding).
fn pack_image(image: &Image, format: PixelFormat) -> Option<Bytes> {
    let width = usize::try_from(image.width().ok()?).unwrap_or(0);
    let height = usize::try_from(image.height().ok()?).unwrap_or(0);
    if width == 0 || height == 0 {
        return None;
    }
    let y = image.plane_data(0).ok()?;
    let y_stride = usize::try_from(image.plane_row_stride(0).ok()?).unwrap_or(0);
    if y_stride < width {
        return None;
    }

    let mut out = Vec::new();
    copy_rows(y, &mut out, width, height, y_stride)?;

    match format {
        PixelFormat::I420 => {
            let u = image.plane_data(1).ok()?;
            let v = image.plane_data(2).ok()?;
            let chroma_stride = usize::try_from(image.plane_row_stride(1).ok()?).unwrap_or(0);
            let chroma_w = width / 2;
            let chroma_rows = height / 2;
            if chroma_stride < chroma_w {
                return None;
            }
            copy_rows(u, &mut out, chroma_w, chroma_rows, chroma_stride)?;
            copy_rows(v, &mut out, chroma_w, chroma_rows, chroma_stride)?;
        }
        PixelFormat::Nv12 => {
            let uv = image.plane_data(1).ok()?;
            let uv_stride = usize::try_from(image.plane_row_stride(1).ok()?).unwrap_or(0);
            let chroma_rows = height / 2;
            if uv_stride < width {
                return None;
            }
            copy_rows(uv, &mut out, width, chroma_rows, uv_stride)?;
        }
        _ => return None,
    }
    Some(Bytes::from(out))
}

fn push_frame(queue: &FrameQueue, format: PixelFormat, data: Bytes, pts: i64) {
    let frame = VideoFrame {
        pts,
        duration: 1,
        width: CAPTURE_WIDTH.unsigned_abs(),
        height: CAPTURE_HEIGHT.unsigned_abs(),
        format,
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
