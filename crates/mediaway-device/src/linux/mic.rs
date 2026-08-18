//! Linux microphone capture: a direct `PipeWire` audio stream (no portal).
//!
//! Unlike [`crate::linux::screencast`]/[`crate::linux::window`], this does **not** go
//! through `xdg-desktop-portal` — regular `PipeWire` clients (`pw-record`,
//! voice-chat apps, …) connect straight to the daemon's local Unix socket to
//! capture audio; there is no portal-mediated consent step for microphone
//! access on desktop Linux the way `ScreenCast` mediates screen capture. See
//! [ADR-0004](adr/0004-pipewire-microphone-capture.md).

#![forbid(unsafe_code)]

use std::collections::VecDeque;
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::audio::{AudioCapture, AudioCaptureConfig};
use crate::{CaptureError, Select};
use mediaway_common::{AudioFrame, Bytes, CodecKind, Rational, StreamInfo};
use pipewire as pw;
use pw::properties::properties;
use pw::spa;

/// Bounded, drop-oldest PCM queue depth — mirrors
/// `mediaway-device-windows` `wasapi.rs`'s `PCM_QUEUE_CAP`.
const PCM_QUEUE_CAP: usize = 64;

struct SharedQueue {
    frames: Mutex<VecDeque<AudioFrame>>,
}

struct MicSession {
    stream_info: StreamInfo,
    queue: Arc<SharedQueue>,
    quit_tx: Option<pw::channel::Sender<()>>,
    worker: Option<JoinHandle<()>>,
}

/// Linux microphone capture via a `PipeWire` `Audio`/`Capture` stream.
///
/// Negotiated as interleaved `F32LE` PCM (the same restriction
/// `mediaway-device-windows` `wasapi.rs` applies — only IEEE float mix
/// formats are accepted, no S16/S32 conversion path this session).
pub struct LinuxMicrophoneCapture {
    inner: Option<MicSession>,
}

impl LinuxMicrophoneCapture {
    /// Open microphone capture for `config`.
    ///
    /// `select` accepts `Select::Default` (the graph's default source) or
    /// `Select::Id(DeviceId::from_pipewire_node_name(..))` (a specific
    /// `PipeWire` node, targeted via `PW_KEY_TARGET_OBJECT` — see ADR-0004
    /// addendum). `Select::NameContains` stays `Unsupported`: this crate has
    /// no `PipeWire` node enumeration to resolve a substring match against
    /// (unlike `Select::Id`, which a caller can already have obtained a
    /// concrete `node.name` for from external tooling); guessing a name
    /// match server-side without one would be a real behavior difference
    /// from every other backend's `NameContains`, not a shortcut.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Unsupported`] for `Select::NameContains`, a
    /// `Select::Id` wrapping a non-`PipeWire` [`DeviceId`](crate::DeviceId),
    /// or a non-`F32` `sample_format`. Returns [`CaptureError::InvalidInput`] for a
    /// zero-denominator time base. Returns [`CaptureError::Backend`] when
    /// the `PipeWire` connection or stream negotiation fails.
    pub fn open(config: &AudioCaptureConfig) -> Result<Self, CaptureError> {
        let target_object = match &config.select {
            Select::Default => None,
            Select::Id(id) => Some(
                id.as_pipewire_node_name()
                    .ok_or(CaptureError::Unsupported)?
                    .to_owned(),
            ),
            Select::NameContains(_) => return Err(CaptureError::Unsupported),
        };
        if config.sample_format != mediaway_common::SampleFormat::F32 {
            return Err(CaptureError::Unsupported);
        }
        if config.time_base.den == 0 {
            return Err(CaptureError::InvalidInput);
        }

        let queue = Arc::new(SharedQueue {
            frames: Mutex::new(VecDeque::new()),
        });
        // clone: worker thread needs its own strong ref to push frames
        let queue_worker = Arc::clone(&queue);
        let time_base = config.time_base;

        let (tx_info, rx_info) = sync_channel::<Result<StreamInfo, CaptureError>>(1);
        let (quit_tx, quit_rx) = pw::channel::channel::<()>();

        let worker = thread::Builder::new()
            .name("mediaway-pw-mic".into())
            .spawn(move || {
                run_pipewire_mic_worker(queue_worker, time_base, target_object, tx_info, quit_rx);
            })
            .map_err(|_| CaptureError::Backend)?;

        let stream_info = rx_info.recv().map_err(|_| CaptureError::Backend)??;

        Ok(Self {
            inner: Some(MicSession {
                stream_info,
                queue,
                quit_tx: Some(quit_tx),
                worker: Some(worker),
            }),
        })
    }
}

impl AudioCapture for LinuxMicrophoneCapture {
    fn stream_info(&self) -> &StreamInfo {
        #[allow(
            clippy::option_if_let_else,
            reason = "map_or_else forces 'static vs 'self lifetime clash"
        )]
        if let Some(s) = self.inner.as_ref() {
            &s.stream_info
        } else {
            closed_audio_info()
        }
    }

    fn poll_frame(&mut self) -> Result<Option<AudioFrame>, CaptureError> {
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

    fn close(&mut self) -> Result<(), CaptureError> {
        let Some(mut session) = self.inner.take() else {
            return Ok(());
        };
        if let Some(tx) = session.quit_tx.take() {
            let _ = tx.send(());
        }
        if let Some(h) = session.worker.take() {
            let _ = h.join();
        }
        Ok(())
    }
}

impl Drop for LinuxMicrophoneCapture {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn closed_audio_info() -> &'static StreamInfo {
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

struct StreamUserData {
    format: spa::param::audio::AudioInfoRaw,
    info_tx: Option<SyncSender<Result<StreamInfo, CaptureError>>>,
    time_base: Rational,
    queue: Arc<SharedQueue>,
    next_pts: i64,
}

/// Worker-thread entry point — mirrors `screencast.rs`'s
/// `run_pipewire_worker`: `LinuxMicrophoneCapture::open` blocks on
/// `rx_info.recv()` for exactly one message (success or failure), sent
/// either from [`on_param_changed`] (success) or here (any setup failure).
fn run_pipewire_mic_worker(
    queue: Arc<SharedQueue>,
    time_base: Rational,
    target_object: Option<String>,
    tx_info: SyncSender<Result<StreamInfo, CaptureError>>,
    quit_rx: pw::channel::Receiver<()>,
) {
    // clone: fallback sender for setup failures before `on_param_changed`
    // takes ownership of the original (see that function).
    let tx_info_fallback = tx_info.clone();
    if let Err(e) = try_run_pipewire_mic(queue, time_base, target_object, tx_info, quit_rx) {
        let _ = tx_info_fallback.send(Err(e));
    }
}

fn try_run_pipewire_mic(
    queue: Arc<SharedQueue>,
    time_base: Rational,
    target_object: Option<String>,
    tx_info: SyncSender<Result<StreamInfo, CaptureError>>,
    quit_rx: pw::channel::Receiver<()>,
) -> Result<(), CaptureError> {
    pw::init();

    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(|_| CaptureError::Backend)?;
    let context =
        pw::context::ContextBox::new(mainloop.loop_(), None).map_err(|_| CaptureError::Backend)?;
    // No fd handoff needed (unlike the portal-mediated `screencast`/`window`
    // sessions) — connects straight to the local `PipeWire` daemon socket.
    let core = context.connect(None).map_err(|_| CaptureError::Backend)?;

    let mainloop_weak = mainloop.downgrade();
    let _quit_listener = quit_rx.attach(mainloop.loop_(), move |()| {
        if let Some(m) = mainloop_weak.upgrade() {
            m.quit();
        }
    });

    let user_data = StreamUserData {
        format: spa::param::audio::AudioInfoRaw::default(),
        info_tx: Some(tx_info),
        time_base,
        queue,
        next_pts: 0,
    };

    let mut props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Communication",
    };
    // `Select::Id(DeviceId::from_pipewire_node_name(..))` targeting — PipeWire resolves this
    // node-name match server-side against the graph; see `LinuxMicrophoneCapture::open`'s doc
    // comment and ADR-0004 addendum.
    if let Some(node_name) = target_object {
        props.insert(*pw::keys::TARGET_OBJECT, node_name);
    }
    let stream = pw::stream::StreamBox::new(&core, "mediaway-microphone", props)
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

            let channels = user_data.format.channels();
            let Some(usable) = usable_pcm_len(chunk_bytes.len(), channels) else {
                return;
            };
            let num_frames = usable / (4 * channels as usize);

            push_frame(
                &user_data.queue,
                user_data.format.rate(),
                channels,
                user_data.next_pts,
                &chunk_bytes[..usable],
            );
            user_data.next_pts = user_data
                .next_pts
                .saturating_add(i64::try_from(num_frames).unwrap_or(0));
        })
        .register()
        .map_err(|_| CaptureError::Backend)?;

    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
    // Leave rate/channels unset — accept whatever the default source graph
    // is running at, read back in `on_param_changed` (same "don't assume"
    // approach `screencast.rs` takes for negotiated video geometry).
    let obj = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };
    let values = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .map_err(|_| CaptureError::Backend)?
    .0
    .into_inner();
    let mut params = [spa::pod::Pod::from_bytes(&values).ok_or(CaptureError::Backend)?];

    stream
        .connect(
            spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(|_| CaptureError::Backend)?;

    // Blocks until `_quit_listener`'s callback calls `mainloop.quit()`
    // (triggered by `LinuxMicrophoneCapture::close` sending on `quit_tx`).
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
    if media_type != spa::param::format::MediaType::Audio
        || media_subtype != spa::param::format::MediaSubtype::Raw
    {
        return;
    }
    if user_data.format.parse(param).is_err() {
        return;
    }

    if let Some(tx) = user_data.info_tx.take() {
        let info = StreamInfo::Audio {
            id: 0,
            codec: CodecKind::RawAudio,
            time_base: user_data.time_base,
            extra_data: Bytes::new(),
            sample_rate: user_data.format.rate(),
            channels: u16::try_from(user_data.format.channels()).unwrap_or(0),
        };
        let _ = tx.send(Ok(info));
    }
}

/// Largest whole-frame-aligned prefix of a `len`-byte `F32LE` interleaved
/// chunk for `channels` channels, or `None` when `channels` is `0` (format
/// not negotiated yet) or the chunk doesn't hold a single whole frame. Pure —
/// unit-testable without a live stream.
fn usable_pcm_len(len: usize, channels: u32) -> Option<usize> {
    if channels == 0 {
        return None;
    }
    let bytes_per_frame = 4usize.checked_mul(channels as usize)?;
    if bytes_per_frame == 0 || len < bytes_per_frame {
        return None;
    }
    Some(len - (len % bytes_per_frame))
}

/// Build one [`AudioFrame`] from a mapped SPA buffer chunk and push it onto
/// the bounded, drop-oldest capture queue (mirrors
/// `mediaway-device-windows` `wasapi.rs`'s `pump_capture_loop` queue
/// discipline).
fn push_frame(queue: &SharedQueue, sample_rate: u32, channels: u32, pts: i64, chunk_bytes: &[u8]) {
    let bytes_per_frame = 4usize.saturating_mul(channels as usize).max(1);
    let num_frames = u64::try_from(chunk_bytes.len() / bytes_per_frame).unwrap_or(0);
    let frame = AudioFrame {
        pts,
        duration: num_frames,
        sample_rate,
        channels: u16::try_from(channels).unwrap_or(0),
        format: mediaway_common::SampleFormat::F32,
        // clone: the PipeWire buffer is requeued to the graph once the
        // `process` callback returns, so the caller-owned `AudioFrame` must
        // outlive it — copying the mapped chunk out is the one copy the
        // buffer-lifetime contract requires (same rationale as
        // `screencast.rs`'s `push_frame`).
        data: Bytes::copy_from_slice(chunk_bytes),
    };
    if let Ok(mut q) = queue.frames.lock() {
        if q.len() >= PCM_QUEUE_CAP {
            let _ = q.pop_front();
        }
        q.push_back(frame);
    }
}

/// Cheap real reachability probe: connect to the local `PipeWire` daemon
/// socket and immediately drop the connection, never calling
/// `mainloop.run()` (so this never blocks on stream negotiation the way a
/// full [`LinuxMicrophoneCapture::open`] would). Used by
/// [`crate::linux::capabilities::support`] for [`crate::DeviceKind::Microphone`]
/// — cheaper than opening a full capture session, same rationale as
/// `mediaway-device-windows` `capabilities.rs`'s `endpoint_support`.
pub(crate) fn probe_daemon_reachable() -> bool {
    pw::init();
    let Ok(mainloop) = pw::main_loop::MainLoopRc::new(None) else {
        return false;
    };
    let Ok(context) = pw::context::ContextBox::new(mainloop.loop_(), None) else {
        return false;
    };
    context.connect(None).is_ok()
}

#[cfg(test)]
#[path = "mic_tests.rs"]
mod tests;
