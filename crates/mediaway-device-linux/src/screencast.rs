//! Linux screen capture: `xdg-desktop-portal` `ScreenCast` session + `PipeWire` stream.

use std::collections::VecDeque;
use std::os::fd::OwnedFd;
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use ashpd::desktop::screencast::SourceType;
use mediaway_common::{
    Bytes, CodecKind, PixelFormat, Rational, StreamInfo, VideoFrame, VideoFrameStorage,
    VideoGeometry,
};
use mediaway_device::{CaptureError, Select};
use mediaway_device_desktop::{
    CaptureOutputPreference, DesktopCaptureSource, DesktopVideoCapture, DesktopVideoCaptureConfig,
};
use pipewire as pw;
use pw::properties::properties;
use pw::spa;

use crate::format::map_spa_video_format;
use crate::portal::{self, PortalStream};

const FRAME_QUEUE_CAP: usize = 4;

struct SharedQueue {
    frames: Mutex<VecDeque<VideoFrame>>,
}

/// Shared session state for both [`LinuxScreenCapture`] and
/// [`crate::window::LinuxWindowCapture`] — the portal `ScreenCast` +
/// `PipeWire` plumbing is identical for `Monitor` and `Window` sources; only
/// the requested [`SourceType`] and the `PipeWire` `MEDIA_ROLE` differ. See
/// [`open_session`].
pub(crate) struct Session {
    stream_info: StreamInfo,
    queue: Arc<SharedQueue>,
    quit_tx: Option<pw::channel::Sender<()>>,
    worker: Option<JoinHandle<()>>,
}

/// Linux screen capture via the portal `ScreenCast` session + a `PipeWire`
/// client stream.
///
/// # Zero-Copy status (CPU copy only this session)
///
/// This backend negotiates a **mappable** SPA buffer (`StreamFlags::MAP_BUFFERS`,
/// `MemPtr`/`MemFd`), not a `DmaBuf` one — `poll_frame` copies the mapped
/// chunk bytes into an owned [`VideoFrameStorage::Cpu`] frame every call. A
/// `DmaBuf`-typed buffer (should the compositor return one anyway) is dropped,
/// never silently re-read as mapped memory. Genuine GPU Zero-Copy (importing
/// the negotiated dma-buf fd into a `VkImage`/`EGLImage` and returning a
/// [`mediaway_common::GpuBufferHandle`]) is deferred — see crate ADR-0001. As a
/// result, [`CaptureOutputPreference::ZeroCopyGpu`] is rejected with
/// [`CaptureError::Unsupported`] rather than silently served from this CPU
/// path.
///
/// **Zero runtime hardware/session verification happened in this development
/// session** — see crate ADR-0001 and the `_or_skip` test in
/// `screencast_tests.rs`.
pub struct LinuxScreenCapture {
    inner: Option<Session>,
}

impl LinuxScreenCapture {
    /// Open a portal `ScreenCast` + `PipeWire` session for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Unsupported`] for non-screen sources, a
    /// non-`Select::Default` selection (the portal's own picker chooses the
    /// monitor interactively — there is no programmatic "pick output *N*"
    /// call, so `Select::Id`/`Select::NameContains` resolution is not
    /// implemented for this backend — ADR-0005 § Deferred), or
    /// [`CaptureOutputPreference::ZeroCopyGpu`] (see struct docs). Returns
    /// other [`CaptureError`] variants ([`portal::map_ashpd_error`]) when the
    /// portal handshake or `PipeWire` connection fails.
    pub fn open(config: &DesktopVideoCaptureConfig) -> Result<Self, CaptureError> {
        let DesktopCaptureSource::Screen { select } = &config.source else {
            return Err(CaptureError::Unsupported);
        };
        if *select != Select::Default {
            return Err(CaptureError::Unsupported);
        }
        let session = open_session(SourceType::Monitor, "Screen", config)?;
        Ok(Self {
            inner: Some(session),
        })
    }
}

/// Run the portal `ScreenCast` handshake for `source_type` and connect a
/// `PipeWire` stream tagged `MEDIA_ROLE => media_role` — the shared plumbing
/// behind both [`LinuxScreenCapture::open`] and
/// [`crate::window::LinuxWindowCapture::open`]. Callers validate their own
/// [`DesktopCaptureSource`] variant before calling this (this function
/// itself only validates [`CaptureOutputPreference`]).
///
/// # Errors
///
/// Returns [`CaptureError::Unsupported`] for
/// [`CaptureOutputPreference::ZeroCopyGpu`] (see struct docs — never silently
/// served from CPU). Returns other [`CaptureError`] variants
/// ([`portal::map_ashpd_error`]) when the portal handshake or `PipeWire`
/// connection fails.
pub(crate) fn open_session(
    source_type: SourceType,
    media_role: &'static str,
    config: &DesktopVideoCaptureConfig,
) -> Result<Session, CaptureError> {
    if config.output != CaptureOutputPreference::CpuFramesOk {
        return Err(CaptureError::Unsupported);
    }

    let PortalStream { node_id, remote_fd } =
        portal::open_portal_stream(source_type).map_err(|e| portal::map_ashpd_error(&e))?;

    let queue = Arc::new(SharedQueue {
        frames: Mutex::new(VecDeque::new()),
    });
    // clone: worker thread needs its own strong ref to push frames
    let queue_worker = Arc::clone(&queue);
    let time_base = config.time_base;

    let (tx_info, rx_info) = sync_channel::<Result<StreamInfo, CaptureError>>(1);
    let (quit_tx, quit_rx) = pw::channel::channel::<()>();

    let worker = thread::Builder::new()
        .name("mediaway-pw-screencast".into())
        .spawn(move || {
            run_pipewire_worker(
                node_id,
                remote_fd,
                media_role,
                queue_worker,
                time_base,
                tx_info,
                quit_rx,
            );
        })
        .map_err(|_| CaptureError::Backend)?;

    let stream_info = rx_info.recv().map_err(|_| CaptureError::Backend)??;

    Ok(Session {
        stream_info,
        queue,
        quit_tx: Some(quit_tx),
        worker: Some(worker),
    })
}

impl Session {
    /// Stream metadata for a live session — [`LinuxScreenCapture`] and
    /// [`crate::window::LinuxWindowCapture`] both delegate their
    /// [`DesktopVideoCapture::stream_info`] to this once `self.inner` is `Some`.
    pub(crate) const fn stream_info(&self) -> &StreamInfo {
        &self.stream_info
    }

    /// Pop the next queued frame, if any — shared [`DesktopVideoCapture::poll_frame`]
    /// body for both capture kinds. Takes `&self`: the queue is behind an
    /// `Arc<Mutex<_>>`, so popping from it needs no exclusive borrow of
    /// `Session` itself.
    pub(crate) fn poll_frame(&self) -> Result<Option<VideoFrame>, CaptureError> {
        let mut q = self
            .queue
            .frames
            .lock()
            .map_err(|_| CaptureError::Backend)?;
        Ok(q.pop_front())
    }

    /// Signal the worker to quit and join it — shared [`DesktopVideoCapture::close`]
    /// body for both capture kinds. Idempotent-safe to call at most once (the
    /// caller `take()`s `Option<Session>` before calling this).
    pub(crate) fn close(&mut self) {
        if let Some(tx) = self.quit_tx.take() {
            let _ = tx.send(());
        }
        if let Some(h) = self.worker.take() {
            let _ = h.join();
        }
    }
}

impl DesktopVideoCapture for LinuxScreenCapture {
    fn stream_info(&self) -> &StreamInfo {
        #[allow(
            clippy::option_if_let_else,
            reason = "map_or_else forces 'static vs 'self lifetime clash"
        )]
        if let Some(inner) = self.inner.as_ref() {
            inner.stream_info()
        } else {
            closed_stream_info()
        }
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, CaptureError> {
        let inner = self.inner.as_ref().ok_or(CaptureError::Closed)?;
        inner.poll_frame()
    }

    fn release_frame(&mut self) -> Result<(), CaptureError> {
        // No GPU handle is ever held (CPU copy path) — nothing to release.
        if self.inner.is_none() {
            return Err(CaptureError::Closed);
        }
        Ok(())
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        let Some(mut session) = self.inner.take() else {
            return Err(CaptureError::Closed);
        };
        session.close();
        Ok(())
    }
}

impl Drop for LinuxScreenCapture {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn closed_stream_info() -> &'static StreamInfo {
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

struct StreamUserData {
    format: spa::param::video::VideoInfoRaw,
    info_tx: Option<SyncSender<Result<StreamInfo, CaptureError>>>,
    time_base: Rational,
    queue: Arc<SharedQueue>,
    next_pts: i64,
}

/// Worker-thread entry point: runs the `PipeWire` main loop until `quit_rx` fires
/// or setup fails. Setup failures are reported through `tx_info` (mirrors the
/// `mediaway-device-windows` WASAPI worker's `notify_err` pattern) since
/// `LinuxScreenCapture::open` blocks on `rx_info.recv()` for exactly one
/// message (success or failure).
fn run_pipewire_worker(
    node_id: u32,
    remote_fd: OwnedFd,
    media_role: &'static str,
    queue: Arc<SharedQueue>,
    time_base: Rational,
    tx_info: SyncSender<Result<StreamInfo, CaptureError>>,
    quit_rx: pw::channel::Receiver<()>,
) {
    // clone: `try_run_pipewire` moves its copy into the stream's user data
    // (sent exactly once, from `param_changed`); this outer copy is the
    // fallback for every setup step that fails before that point.
    let tx_info_fallback = tx_info.clone();
    if let Err(e) = try_run_pipewire(
        node_id, remote_fd, media_role, queue, time_base, tx_info, quit_rx,
    ) {
        let _ = tx_info_fallback.send(Err(e));
    }
}

fn try_run_pipewire(
    node_id: u32,
    remote_fd: OwnedFd,
    media_role: &'static str,
    queue: Arc<SharedQueue>,
    time_base: Rational,
    tx_info: SyncSender<Result<StreamInfo, CaptureError>>,
    quit_rx: pw::channel::Receiver<()>,
) -> Result<(), CaptureError> {
    pw::init();

    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(|_| CaptureError::Backend)?;
    let context =
        pw::context::ContextBox::new(mainloop.loop_(), None).map_err(|_| CaptureError::Backend)?;
    let core = context
        .connect_fd(remote_fd, None)
        .map_err(|_| CaptureError::Backend)?;

    let mainloop_weak = mainloop.downgrade();
    let _quit_listener = quit_rx.attach(mainloop.loop_(), move |()| {
        if let Some(m) = mainloop_weak.upgrade() {
            m.quit();
        }
    });

    let user_data = StreamUserData {
        format: spa::param::video::VideoInfoRaw::default(),
        info_tx: Some(tx_info),
        time_base,
        queue,
        next_pts: 0,
    };

    // `MEDIA_ROLE` varies by caller (`"Screen"` / `"Window"`) so it's a
    // runtime `insert` rather than a `properties!` macro literal (the macro
    // only accepts compile-time key/value literals).
    let mut stream_props = properties! {
        *pw::keys::MEDIA_TYPE => "Video",
        *pw::keys::MEDIA_CATEGORY => "Capture",
    };
    stream_props.insert(*pw::keys::MEDIA_ROLE, media_role);

    let stream = pw::stream::StreamBox::new(&core, "mediaway-screencast", stream_props)
        .map_err(|_| CaptureError::Backend)?;

    let _listener = stream
        .add_local_listener_with_user_data(user_data)
        .param_changed(|_stream, user_data, id, param| on_param_changed(user_data, id, param))
        .process(|stream, user_data| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buffer.datas_mut();
            let Some(data) = datas.first_mut() else {
                return;
            };
            if data.type_() == spa::buffer::DataType::DmaBuf {
                // Negotiated a DMA-BUF-backed buffer despite `MAP_BUFFERS` — GPU
                // Zero-Copy import is out of scope this session (see struct
                // rustdoc / ADR-0001). Drop the frame rather than mis-read the
                // fd as mapped memory.
                return;
            }

            let chunk_offset = usize::try_from(data.chunk().offset()).unwrap_or(0);
            let chunk_size = usize::try_from(data.chunk().size()).unwrap_or(0);
            let Some(mapped) = data.data() else {
                return;
            };
            let Some(chunk_bytes) =
                mapped.get(chunk_offset..chunk_offset.saturating_add(chunk_size))
            else {
                return;
            };
            let Some(format) = map_spa_video_format(user_data.format.format()) else {
                return;
            };
            let size = user_data.format.size();

            push_frame(
                &user_data.queue,
                format,
                size.width,
                size.height,
                user_data.next_pts,
                chunk_bytes,
            );
            user_data.next_pts = user_data.next_pts.saturating_add(1);
        })
        .register()
        .map_err(|_| CaptureError::Backend)?;

    let values = build_format_pod_values()?;
    let mut params = [spa::pod::Pod::from_bytes(&values).ok_or(CaptureError::Backend)?];

    stream
        .connect(
            spa::utils::Direction::Input,
            Some(node_id),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(|_| CaptureError::Backend)?;

    // Blocks until `_quit_listener`'s callback calls `mainloop.quit()`
    // (triggered by `LinuxScreenCapture::close` sending on `quit_tx`).
    mainloop.run();
    Ok(())
}

fn on_param_changed(user_data: &mut StreamUserData, id: u32, param: Option<&spa::pod::Pod>) {
    let Some(param) = param else {
        return;
    };
    if id != spa::param::ParamType::Format.as_raw() {
        return;
    }
    let Ok((media_type, media_subtype)) = spa::param::format_utils::parse_format(param) else {
        return;
    };
    if media_type != spa::param::format::MediaType::Video
        || media_subtype != spa::param::format::MediaSubtype::Raw
    {
        return;
    }
    if user_data.format.parse(param).is_err() {
        return;
    }

    if let Some(tx) = user_data.info_tx.take() {
        let size = user_data.format.size();
        let info = StreamInfo::Video {
            id: 0,
            codec: CodecKind::RawVideo,
            time_base: user_data.time_base,
            geometry: VideoGeometry {
                width: size.width,
                height: size.height,
            },
            extra_data: Bytes::new(),
        };
        let _ = tx.send(Ok(info));
    }
}

/// Build one [`VideoFrame`] from a mapped SPA buffer chunk and push it onto
/// the bounded, drop-oldest capture queue (mirrors the `mediaway-device-windows`
/// WASAPI worker's queue discipline).
fn push_frame(
    queue: &SharedQueue,
    format: PixelFormat,
    width: u32,
    height: u32,
    pts: i64,
    chunk_bytes: &[u8],
) {
    let frame = VideoFrame {
        pts,
        duration: 1,
        width,
        height,
        format,
        storage: VideoFrameStorage::Cpu {
            // clone: the PipeWire buffer is requeued to the compositor once
            // the `process` callback returns, so the caller-owned `VideoFrame`
            // must outlive it — copying the mapped chunk out is the one copy
            // the buffer-lifetime contract requires (see `LinuxScreenCapture`
            // rustdoc's Zero-Copy status note; this is the CPU-copy path, not
            // GPU ZC).
            data: Bytes::copy_from_slice(chunk_bytes),
        },
    };
    if let Ok(mut q) = queue.frames.lock() {
        if q.len() >= FRAME_QUEUE_CAP {
            let _ = q.pop_front();
        }
        q.push_back(frame);
    }
}

/// Build the serialized `SPA_PARAM_EnumFormat` object offered to
/// `stream.connect` — kept to exactly the formats [`map_spa_video_format`]
/// maps (see that function's rustdoc: this pairing is hand-maintained, not
/// compiler-checked across the proc-macro boundary). Widening one without the
/// other means `poll_frame` either rejects a format the server could have
/// offered, or `param_changed` negotiates one `map_spa_video_format` still
/// returns `None` for.
fn build_format_pod_values() -> Result<Vec<u8>, CaptureError> {
    let obj = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::RGBx,
            spa::param::video::VideoFormat::RGBA,
            spa::param::video::VideoFormat::I420,
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle {
                width: 320,
                height: 240
            },
            spa::utils::Rectangle {
                width: 1,
                height: 1
            },
            spa::utils::Rectangle {
                width: 8192,
                height: 8192
            }
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction { num: 30, denom: 1 },
            spa::utils::Fraction { num: 0, denom: 1 },
            spa::utils::Fraction {
                num: 1000,
                denom: 1
            }
        ),
    );
    Ok(spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .map_err(|_| CaptureError::Backend)?
    .0
    .into_inner())
}

#[cfg(test)]
#[path = "screencast_tests.rs"]
mod tests;
