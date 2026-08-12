//! Android screen capture via `MediaProjection` + a native `AImageReader`/`Surface` bridge.
//! See [ADR-0003](adr/android/0003-mediaprojection-jni-screen-capture.md).
//!
//! Unlike `camera.rs`/`mic.rs`, this domain cannot be a same-shape `open(&DesktopVideoCaptureConfig)`
//! API — `MediaProjection` only exists as a Java object obtained through a consent flow only a
//! JVM `Activity` can run (`MediaProjectionManager.createScreenCaptureIntent()` →
//! `startActivityForResult` → `onActivityResult` → `getMediaProjection(resultCode, data)`; see
//! the ADR for why `android-activity`'s stock `AndroidApp` cannot do this). The host app runs
//! that consent flow itself, converts the resulting `MediaProjection` to a JNI **global**
//! reference, and hands the raw bits + a `JavaVM*` to [`AndroidScreenCaptureConfig`]. This
//! module owns everything from there: `ImageReader` → `NativeWindow` → `Surface` →
//! `createVirtualDisplay` → frame pump → `close()` → `mediaProjection.stop()`.
//!
//! # Two different `jni-sys` crate versions bridged via raw pointer casts
//!
//! `ndk` 0.9's `NativeWindow::to_surface`/`from_surface` take/return `jni-sys` **0.3** types
//! (`ndk`'s own pinned dependency); this crate's `jni` 0.22 dependency is built on `jni-sys`
//! **0.4**. Both crate versions model the exact same JNI ABI (`JNIEnv`/`jobject` are opaque
//! pointers whose shape is fixed by the JVM specification, not by either Rust crate), so a raw
//! pointer `.cast()` between them is a real, deliberate bridge — not a coincidental unsafe
//! shortcut — and is the only way to call `to_surface` at all without vendoring a second
//! `jni`-major-version dependency just for this one FFI boundary. Every such cast below has an
//! explicit `# Safety`/`SAFETY` note; this bridging technique is unverified against a real
//! device this session (see the crate's `android` CI job — even a green compile cannot confirm
//! runtime ABI compatibility, only that both types have the same size/layout at compile time).
//!
//! **Zero compile verification, zero real-hardware verification.**

#![allow(unsafe_code)]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::android::AndroidScreenCaptureConfig;
use crate::desktop::DesktopVideoCapture;
use crate::{CaptureError, android::jni_util};
use jni::objects::{JObject, JValue};
use jni::{jni_sig, jni_str};
use mediaway_common::{
    Bytes, CodecKind, PixelFormat, Rational, StreamInfo, VideoFrame, VideoFrameStorage,
    VideoGeometry,
};
use ndk::hardware_buffer::HardwareBufferUsage;
use ndk::media::image_reader::{AcquireResult, ImageFormat, ImageReader};

/// Bounded, drop-oldest delivered-frame queue depth — mirrors `linux::screencast`/`camera.rs`.
const FRAME_QUEUE_CAP: usize = 4;

/// `AImageReader` internal buffer count.
const MAX_IMAGES: i32 = 4;

/// How often the worker polls `acquire_latest_image` and rechecks the stop flag.
const POLL_INTERVAL: Duration = Duration::from_millis(8);

struct FrameQueue {
    frames: Mutex<VecDeque<VideoFrame>>,
}

struct ScreenSession {
    stream_info: StreamInfo,
    queue: Arc<FrameQueue>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

/// Android screen capture session (`MediaProjection` + native `AImageReader`, CPU RGBA frames).
/// See module docs for the host-app contract this requires.
pub struct AndroidScreenCapture {
    inner: Option<ScreenSession>,
}

impl AndroidScreenCapture {
    /// Open screen capture for `config`. See module docs and
    /// [`AndroidScreenCaptureConfig`] for the required host-app-supplied `MediaProjection`/
    /// `JavaVM` handles.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::InvalidInput`] for zero `width`/`height` or a zero-denominator
    /// time base. Returns [`CaptureError::Backend`] on `ImageReader`/`Surface`/JNI failure
    /// (including a thrown Java exception from `createVirtualDisplay`, e.g. the Android 14+
    /// single-use-consent `SecurityException` documented on [`AndroidScreenCaptureConfig::media_projection`]).
    pub fn open(config: &AndroidScreenCaptureConfig) -> Result<Self, CaptureError> {
        if config.width == 0 || config.height == 0 || config.time_base.den == 0 {
            return Err(CaptureError::InvalidInput);
        }

        let queue = Arc::new(FrameQueue {
            frames: Mutex::new(VecDeque::new()),
        });
        // clone: Arc share with screencast worker thread
        let queue_worker = Arc::clone(&queue);
        let stop = Arc::new(AtomicBool::new(false));
        // clone: Arc share with screencast worker thread
        let stop_worker = Arc::clone(&stop);
        let cfg = *config;

        let (tx_info, rx_info) = std::sync::mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("mediaway-mediaprojection".into())
            .spawn(move || {
                run_screencast_worker(&cfg, &queue_worker, &stop_worker, &tx_info);
            })
            .map_err(|_| CaptureError::Backend)?;

        let stream_info = rx_info.recv().map_err(|_| CaptureError::Backend)??;

        Ok(Self {
            inner: Some(ScreenSession {
                stream_info,
                queue,
                stop,
                worker: Some(worker),
            }),
        })
    }
}

impl DesktopVideoCapture for AndroidScreenCapture {
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

    /// Signals the worker's stop flag and joins it. The worker thread calls
    /// `mediaProjection.stop()` (JNI) and drops the native `ImageReader`/global reference
    /// before it exits — see [`run_screencast_session`].
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

impl Drop for AndroidScreenCapture {
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

fn run_screencast_worker(
    cfg: &AndroidScreenCaptureConfig,
    queue: &FrameQueue,
    stop: &AtomicBool,
    tx_info: &SyncSender<Result<StreamInfo, CaptureError>>,
) {
    let vm_ptr = cfg.java_vm.get() as *mut jni::sys::JavaVM;
    let media_projection_raw = cfg.media_projection.get() as jni::sys::jobject;

    // SAFETY: `AndroidScreenCaptureConfig`'s own contract (documented on its fields) is that
    // `java_vm` is a valid, live `JavaVM*` and `media_projection` is an already-global JNI
    // reference to a live `MediaProjection`, both outliving this call.
    let result: Result<(), CaptureError> = unsafe {
        jni_util::with_attached_env(vm_ptr, |env| {
            run_screencast_session(env, media_projection_raw, cfg, queue, stop, tx_info)
        })
    };
    if let Err(e) = result {
        let _ = tx_info.send(Err(e));
    }
}

/// Runs the whole session lifecycle — `createVirtualDisplay`, the poll loop, and
/// `mediaProjection.stop()` teardown — on the one worker thread attached to `env`'s `JavaVM`
/// for the whole call. Sends `Ok(stream_info)` on `tx_info` exactly once, on success; a setup
/// failure is returned (not sent) so the caller ([`run_screencast_worker`]) sends it instead —
/// avoids a double-send race between this function and its caller.
fn run_screencast_session(
    env: &mut jni::Env,
    media_projection_raw: jni::sys::jobject,
    cfg: &AndroidScreenCaptureConfig,
    queue: &FrameQueue,
    stop: &AtomicBool,
    tx_info: &SyncSender<Result<StreamInfo, CaptureError>>,
) -> Result<(), CaptureError> {
    // SAFETY: caller's contract (`run_screencast_worker`'s own `# Safety`) guarantees
    // `media_projection_raw` is a live, already-global JNI reference this function now owns —
    // `Global`'s `Drop` will `DeleteGlobalRef` it when this function returns.
    let media_projection = unsafe { env.global_from_raw::<JObject>(media_projection_raw) };

    let width = i32::try_from(cfg.width).map_err(|_| CaptureError::InvalidInput)?;
    let height = i32::try_from(cfg.height).map_err(|_| CaptureError::InvalidInput)?;
    let density_dpi = i32::try_from(cfg.density_dpi).map_err(|_| CaptureError::InvalidInput)?;

    let reader = ImageReader::new_with_usage(
        width,
        height,
        ImageFormat::RGBA_8888,
        HardwareBufferUsage::CPU_READ_OFTEN,
        MAX_IMAGES,
    )
    .map_err(|_| CaptureError::Backend)?;
    let window = reader.window().map_err(|_| CaptureError::Backend)?;

    // SAFETY: `env.get_raw()` returns this thread's live `jni-sys 0.4` `*mut JNIEnv`; `ndk`
    // 0.9's `to_surface` expects a `jni-sys 0.3` `*mut JNIEnv` — module docs § "Two different
    // `jni-sys` crate versions bridged via raw pointer casts" justify this reinterpret-cast as
    // both crates modeling the same fixed JNI ABI, not a coincidental shortcut. `window`
    // outlives this call (owned by `reader`, kept alive below).
    let surface_raw = unsafe { window.to_surface(env.get_raw().cast()) };
    drop(window);
    if surface_raw.is_null() {
        return Err(CaptureError::Backend);
    }
    // SAFETY: `surface_raw` is a valid local JNI reference just returned by `to_surface` above,
    // scoped to this JNI stack frame — matches `JObject::from_raw`'s own contract.
    let surface = unsafe { JObject::from_raw(env, surface_raw.cast()) };

    let name = env
        .new_string("mediaway-screen")
        .map_err(CaptureError::from)?;
    // Bound to locals (not inline temporaries) — `args` below borrows them, and a temporary
    // created inside the array-literal statement itself would be dropped at that statement's
    // end, before `args` is used by `call_method` in the next statement.
    let name_obj = JObject::from(name);
    let null_obj = JObject::null();
    let args = [
        JValue::Object(&name_obj),
        JValue::Int(width),
        JValue::Int(height),
        JValue::Int(density_dpi),
        JValue::Int(cfg.flags),
        JValue::Object(&surface),
        JValue::Object(&null_obj),
        JValue::Object(&null_obj),
    ];
    let sig = jni_sig!(
        "(Ljava/lang/String;IIIILandroid/view/Surface;Landroid/hardware/display/VirtualDisplay$Callback;Landroid/os/Handler;)Landroid/hardware/display/VirtualDisplay;"
    );
    let virtual_display = env
        .call_method(
            media_projection.as_obj(),
            jni_str!("createVirtualDisplay"),
            sig,
            &args,
        )
        .map_err(CaptureError::from)?;
    if virtual_display.l().map_err(CaptureError::from)?.is_null() {
        return Err(CaptureError::Backend);
    }

    let info = StreamInfo::Video {
        id: 0,
        codec: CodecKind::RawVideo,
        time_base: cfg.time_base,
        geometry: VideoGeometry {
            width: cfg.width,
            height: cfg.height,
        },
        extra_data: Bytes::new(),
    };
    let _ = tx_info.send(Ok(info));

    let mut pts: i64 = 0;
    while !stop.load(Ordering::Relaxed) {
        match reader.acquire_latest_image() {
            Ok(AcquireResult::Image(image)) => {
                if let Some(data) = pack_rgba_image(&image, cfg.width, cfg.height) {
                    push_frame(queue, cfg.width, cfg.height, data, pts);
                    pts = pts.saturating_add(1);
                }
            }
            Ok(_) | Err(_) => thread::sleep(POLL_INTERVAL),
        }
    }

    let _: jni::errors::Result<jni::objects::JValueOwned<'_>> = env.call_method(
        media_projection.as_obj(),
        jni_str!("stop"),
        jni_sig!("()V"),
        &[],
    );
    // `media_projection`'s `Drop` (`Global`) deletes the global reference; `reader`'s `Drop`
    // (`AImageReader_delete`) releases the native side.
    Ok(())
}

fn pack_rgba_image(
    image: &ndk::media::image_reader::Image,
    width: u32,
    height: u32,
) -> Option<Bytes> {
    let data = image.plane_data(0).ok()?;
    let stride = image.plane_row_stride(0).ok()?.max(0) as usize;
    let row_bytes = (width as usize).checked_mul(4)?;
    if stride < row_bytes {
        return None;
    }
    let mut out = Vec::with_capacity(row_bytes.checked_mul(height as usize)?);
    for row in 0..height as usize {
        let start = row.checked_mul(stride)?;
        let end = start.checked_add(row_bytes)?;
        out.extend_from_slice(data.get(start..end)?);
    }
    Some(Bytes::from(out))
}

fn push_frame(queue: &FrameQueue, width: u32, height: u32, data: Bytes, pts: i64) {
    let frame = VideoFrame {
        pts,
        duration: 1,
        width,
        height,
        format: PixelFormat::Rgba8,
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
#[path = "screencast_tests.rs"]
mod tests;
