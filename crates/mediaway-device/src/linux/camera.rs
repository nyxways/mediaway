//! Linux camera capture via `Video4Linux2` (`v4l` crate, `VIDIOC_*` ioctls).
//!
//! [`LinuxCameraCapture::open`] enumerates `/dev/video*` nodes that report
//! `V4L2_CAP_VIDEO_CAPTURE` (`VIDIOC_QUERYCAP`, filtering out
//! metadata-capture-only sibling nodes many UVC webcams also expose),
//! activates the node at [`CameraCaptureConfig`]'s `select` ordinal index, and
//! negotiates a raw pixel format the driver already advertises via
//! `VIDIOC_ENUM_FMT` — preferring `YUYV` (the most common raw output of real
//! UVC webcams), then `NV12`, then `YU12` (planar I420). Unlike the Windows
//! Media Foundation backend ([`crate` sibling] `mediaway-device-windows`'s
//! `camera.rs`), there is no built-in video-processor conversion here: a
//! webcam that only offers `MJPG` (compressed) or another raw layout not in
//! that list has no supported format this session — see
//! [ADR-0002](adr/0002-v4l2-camera-capture.md) § Format coverage.
//!
//! # No `unsafe` in this module
//!
//! The `v4l` crate's whole capture surface used here (`Device`, `Capture`,
//! `MmapStream`, `CaptureStream`) is safe Rust — `mmap`/ioctl `unsafe` lives
//! entirely inside `v4l`/`v4l2-sys-mit`. This module stays
//! `#![forbid(unsafe_code)]`, unlike the crate's `screencast.rs`/`window.rs`/
//! `mic.rs` (which call into `pipewire`'s buffer-pointer APIs directly).
//!
//! **Zero runtime hardware verification happened in this development
//! session** — see crate ADR-0002 and the `_or_skip` tests in
//! `camera_tests.rs`. WSL2 has no `/dev/video*` nodes at all (confirmed this
//! session).

#![forbid(unsafe_code)]

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::camera::{CameraCapture, CameraCaptureConfig, CaptureOutputPreference};
use crate::{CaptureError, Select};
use mediaway_common::{
    Bytes, CodecKind, PixelFormat, Rational, StreamInfo, VideoFrame, VideoFrameStorage,
    VideoGeometry,
};
use v4l::buffer::Type as V4lBufferType;
use v4l::capability::Flags as V4lCapabilityFlags;
use v4l::io::mmap::Stream as MmapStream;
use v4l::io::traits::CaptureStream as _;
use v4l::video::Capture as _;
use v4l::{Device as V4lDevice, Format as V4lFormat, FourCC};

/// Bounded, drop-oldest delivered-frame queue depth — mirrors
/// `mediaway-device-windows` `camera.rs`'s `FRAME_QUEUE_CAP`.
const FRAME_QUEUE_CAP: usize = 4;

/// `v4l` mmap arena buffer count (kernel-side ring), independent of the
/// delivered-frame queue above.
const CAPTURE_BUFFER_COUNT: u32 = 4;

/// How long `VIDIOC_DQBUF` blocks before `stream.next()` returns
/// `io::ErrorKind::TimedOut`, letting the worker recheck the stop flag —
/// mirrors the Windows backend's "close can wait up to one frame interval"
/// contract (see `close()` docs below), without a real async cancel path.
const STREAM_POLL_TIMEOUT: Duration = Duration::from_millis(200);

/// Fallback capture size when a node's current format reports `0x0` (never
/// configured yet) — chosen only to give `VIDIOC_S_FMT` a starting point; the
/// driver's `set_format` response (read back and used from then on) is what
/// actually determines the negotiated geometry.
const FALLBACK_WIDTH: u32 = 640;
const FALLBACK_HEIGHT: u32 = 480;

struct FrameQueue {
    frames: Mutex<VecDeque<VideoFrame>>,
}

struct CameraSession {
    stream_info: StreamInfo,
    queue: Arc<FrameQueue>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

/// Linux camera capture session (V4L2 `mmap` streaming I/O, CPU frames).
///
/// See the module docs for format coverage and the Zero-Copy status (CPU-only
/// this session — no DMA-BUF import, matching `screencast.rs`'s status).
pub struct LinuxCameraCapture {
    inner: Option<CameraSession>,
}

impl LinuxCameraCapture {
    /// Open V4L2 camera capture for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Unsupported`] for a non-`Select::Default`
    /// selection (`Select::Id`/`Select::NameContains` resolution is not
    /// implemented for this backend yet — ADR-0005 § Deferred), the
    /// [`CaptureOutputPreference::ZeroCopyGpu`] preference
    /// (not implemented — see module docs), or when the device offers none
    /// of `YUYV`/`NV12`/`YU12`. Returns [`CaptureError::InvalidInput`] when
    /// no capture-capable node exists at ordinal `0`. Returns
    /// [`CaptureError::AccessDenied`] when opening the node fails with
    /// `EACCES` (not a member of the `video` group, or restrictive device
    /// permissions). Returns [`CaptureError::Backend`] on other V4L2/ioctl
    /// failures.
    pub fn open(config: &CameraCaptureConfig) -> Result<Self, CaptureError> {
        if config.select != Select::Default {
            return Err(CaptureError::Unsupported);
        }
        let device = 0usize;
        if config.output != CaptureOutputPreference::CpuFramesOk {
            return Err(CaptureError::Unsupported);
        }

        let queue = Arc::new(FrameQueue {
            frames: Mutex::new(VecDeque::new()),
        });
        let stop = Arc::new(AtomicBool::new(false));
        // clone: Arc share with camera worker thread
        let queue_worker = Arc::clone(&queue);
        // clone: Arc share with camera worker thread
        let stop_worker = Arc::clone(&stop);
        let time_base = config.time_base;

        let (tx_info, rx_info) = std::sync::mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("mediaway-v4l2-camera".into())
            .spawn(move || {
                run_camera_worker(device, time_base, &queue_worker, &stop_worker, &tx_info);
            })
            .map_err(|_| CaptureError::Backend)?;

        let stream_info = rx_info.recv().map_err(|_| CaptureError::Backend)??;

        Ok(Self {
            inner: Some(CameraSession {
                stream_info,
                queue,
                stop,
                worker: Some(worker),
            }),
        })
    }
}

impl CameraCapture for LinuxCameraCapture {
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

    /// Signals the worker's stop flag and joins it. `VIDIOC_DQBUF` blocks up
    /// to [`STREAM_POLL_TIMEOUT`] before the worker notices `stop` — same
    /// "wait up to one frame interval" contract `mediaway-device-windows`
    /// `camera.rs` documents for its synchronous `ReadSample` pump.
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

impl Drop for LinuxCameraCapture {
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

fn run_camera_worker(
    device_index: usize,
    time_base: Rational,
    queue: &FrameQueue,
    stop: &AtomicBool,
    tx_info: &SyncSender<Result<StreamInfo, CaptureError>>,
) {
    let (device, stream_info, format, width, height, stride) =
        match open_and_negotiate(device_index, time_base) {
            Ok(v) => v,
            Err(e) => {
                let _ = tx_info.send(Err(e));
                return;
            }
        };

    let Ok(mut stream) =
        MmapStream::with_buffers(&device, V4lBufferType::VideoCapture, CAPTURE_BUFFER_COUNT)
    else {
        let _ = tx_info.send(Err(CaptureError::Backend));
        return;
    };
    stream.set_timeout(STREAM_POLL_TIMEOUT);

    let _ = tx_info.send(Ok(stream_info));

    let mut pts: i64 = 0;
    while !stop.load(Ordering::Relaxed) {
        match stream.next() {
            Ok((bytes, _meta)) => {
                if let Some(data) = pack_frame_bytes(bytes, format, width, height, stride) {
                    push_frame(queue, format, width, height, pts, data);
                    pts = pts.saturating_add(1);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => break,
        }
    }
    // `stream`'s `Drop` issues `VIDIOC_STREAMOFF` — see ADR-0002 § Dependency
    // caveat for a real footgun found in that path.
}

/// Enumerate + open the node at `device_index`, negotiate a supported pixel
/// format, and read back the negotiated geometry/stride. Returns the opened
/// [`V4lDevice`] so the caller can build an [`MmapStream`] from it in the
/// same stack frame (`Stream`'s lifetime parameter ties to its `&Device`
/// borrow — see ADR-0002 § Why one function, not a split helper).
fn open_and_negotiate(
    device_index: usize,
    time_base: Rational,
) -> Result<(V4lDevice, StreamInfo, PixelFormat, u32, u32, u32), CaptureError> {
    let nodes = enumerate_capture_nodes();
    let path = nodes.get(device_index).ok_or(CaptureError::InvalidInput)?;

    let device = V4lDevice::with_path(path).map_err(|e| map_io_error(&e))?;

    let available: Vec<[u8; 4]> = device
        .enum_formats()
        .map_err(|e| map_io_error(&e))?
        .into_iter()
        .map(|d| d.fourcc.repr)
        .collect();
    let (format, fourcc) = pick_capture_format(&available).ok_or(CaptureError::Unsupported)?;

    let current = device.format().map_err(|e| map_io_error(&e))?;
    let (width, height) = if current.width > 0 && current.height > 0 {
        (current.width, current.height)
    } else {
        (FALLBACK_WIDTH, FALLBACK_HEIGHT)
    };

    let requested = V4lFormat::new(width, height, FourCC::new(&fourcc));
    let negotiated = device
        .set_format(&requested)
        .map_err(|e| map_io_error(&e))?;
    if negotiated.fourcc != requested.fourcc {
        // The driver silently substituted a different pixel layout than the
        // one `pack_frame_bytes` below is about to assume — reject rather
        // than mis-read bytes (same "never guess" rule as
        // `format::map_spa_video_format`).
        return Err(CaptureError::Unsupported);
    }
    if negotiated.width == 0 || negotiated.height == 0 {
        return Err(CaptureError::Backend);
    }
    let stride = if negotiated.stride > 0 {
        negotiated.stride
    } else {
        min_stride(format, negotiated.width)
    };

    let info = StreamInfo::Video {
        id: 0,
        codec: CodecKind::RawVideo,
        time_base,
        geometry: VideoGeometry {
            width: negotiated.width,
            height: negotiated.height,
        },
        extra_data: Bytes::new(),
    };
    Ok((
        device,
        info,
        format,
        negotiated.width,
        negotiated.height,
        stride,
    ))
}

/// Capture-capable (`V4L2_CAP_VIDEO_CAPTURE`) `/dev/video*` nodes, numerically
/// ordered by node index (`video0`, `video1`, …, `video10`, not lexical
/// `video1` < `video10` < `video2`) and ordinal-indexed the same way
/// [`CameraCaptureConfig`]'s `select` field is. Filters out the
/// metadata-capture-only sibling node many UVC webcams also expose
/// alongside their real capture node — `VIDIOC_QUERYCAP` is the only
/// reliable way to tell them apart (name/order are not).
fn enumerate_capture_nodes() -> Vec<PathBuf> {
    let mut nodes = v4l::context::enum_devices();
    nodes.sort_by_key(v4l::context::Node::index);
    nodes
        .into_iter()
        .filter_map(|node| {
            let device = V4lDevice::with_path(node.path()).ok()?;
            let caps = device.query_caps().ok()?;
            caps.capabilities
                .contains(V4lCapabilityFlags::VIDEO_CAPTURE)
                .then(|| node.path().to_path_buf())
        })
        .collect()
}

fn map_io_error(e: &std::io::Error) -> CaptureError {
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => CaptureError::AccessDenied,
        std::io::ErrorKind::NotFound => CaptureError::InvalidInput,
        _ => CaptureError::Backend,
    }
}

/// Preference order: `YUYV` (packed 4:2:2 — the common raw UVC output) >
/// `NV12` (semi-planar 4:2:0) > `YU12`/I420 (planar 4:2:0). Pure — no V4L2
/// I/O — so the priority decision is unit-testable without a device, same
/// rationale as `mediaway-device-windows` `camera.rs`'s
/// `preferred_subtype_order`.
const PREFERRED_FOURCCS: [(PixelFormat, [u8; 4]); 3] = [
    (PixelFormat::Yuyv, *b"YUYV"),
    (PixelFormat::Nv12, *b"NV12"),
    (PixelFormat::I420, *b"YU12"),
];

fn pick_capture_format(available: &[[u8; 4]]) -> Option<(PixelFormat, [u8; 4])> {
    PREFERRED_FOURCCS
        .into_iter()
        .find(|(_, fourcc)| available.contains(fourcc))
}

/// Minimum tightly packed row stride for `format` at `width`, used only when
/// the driver reports `bytesperline == 0` (some drivers leave it unset for
/// certain formats/nodes).
const fn min_stride(format: PixelFormat, width: u32) -> u32 {
    match format {
        PixelFormat::Yuyv => width * 2,
        _ => width,
    }
}

/// Copy `rows` rows of `row_bytes` bytes each out of `src` at `stride`
/// spacing, appending them tightly packed (no padding) to `out`. `None` when
/// `src` is too short for the requested rows — never reads past the buffer.
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

/// Build one tightly packed frame buffer from a raw `mmap`-ed V4L2 buffer
/// (`src`), honoring the negotiated row `stride` (which may exceed the tight
/// row width — driver alignment padding). Pure — a plain byte-slice in, byte
/// buffer out — so this is unit-testable with synthetic buffers, unlike the
/// Windows Media Foundation backend's pointer-based equivalent
/// (`copy_2d_nv12`/`copy_2d_packed`), which needs a live `IMF2DBuffer`.
///
/// `I420`'s chroma planes are assumed to use `stride / 2` with no additional
/// padding of their own — the single-planar V4L2 API
/// ([`v4l2_pix_format`](https://docs.kernel.org/userspace-api/media/v4l/pixfmt-v4l2.html))
/// only reports one `bytesperline` (the luma plane's), not independent
/// per-plane strides; `stride / 2` is the near-universal driver convention
/// for this layout, not a value the kernel API exposes directly — documented
/// approximation, same honesty rule as `format::map_spa_video_format`'s
/// `BGRx`/`RGBx` note.
fn pack_frame_bytes(
    src: &[u8],
    format: PixelFormat,
    width: u32,
    height: u32,
    stride: u32,
) -> Option<Bytes> {
    let (w, h, stride) = (width as usize, height as usize, stride as usize);
    if w == 0 || h == 0 || stride == 0 {
        return None;
    }
    let mut out = Vec::new();
    match format {
        PixelFormat::Yuyv if stride >= w * 2 => {
            copy_rows(src, &mut out, w * 2, h, stride)?;
        }
        PixelFormat::Nv12 if stride >= w => {
            copy_rows(src, &mut out, w, h, stride)?;
            let chroma = src.get(stride.checked_mul(h)?..)?;
            copy_rows(chroma, &mut out, w, h / 2, stride)?;
        }
        PixelFormat::I420 if stride >= w => {
            copy_rows(src, &mut out, w, h, stride)?;
            let chroma_stride = stride / 2;
            let chroma_w = w / 2;
            let chroma_rows = h / 2;
            let u_plane = src.get(stride.checked_mul(h)?..)?;
            copy_rows(u_plane, &mut out, chroma_w, chroma_rows, chroma_stride)?;
            let v_plane = u_plane.get(chroma_stride.checked_mul(chroma_rows)?..)?;
            copy_rows(v_plane, &mut out, chroma_w, chroma_rows, chroma_stride)?;
        }
        _ => return None,
    }
    Some(Bytes::from(out))
}

fn push_frame(
    queue: &FrameQueue,
    format: PixelFormat,
    width: u32,
    height: u32,
    pts: i64,
    data: Bytes,
) {
    let frame = VideoFrame {
        pts,
        duration: 1,
        width,
        height,
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

/// Real device paths (not just names) capture-capable V4L2 nodes report on
/// this machine, ordinal-indexed the same way [`enumerate_capture_nodes`]
/// is. Used by this crate's hardware-gated tests to check whether a camera
/// exists at all before attempting [`LinuxCameraCapture::open`].
pub(crate) fn enumerate_camera_paths() -> Vec<PathBuf> {
    enumerate_capture_nodes()
}

#[cfg(test)]
#[path = "camera_tests.rs"]
mod tests;
