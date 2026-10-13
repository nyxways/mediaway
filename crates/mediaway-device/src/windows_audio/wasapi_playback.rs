//! WASAPI shared-mode render (playback) — mirrors `wasapi.rs` in the
//! opposite data direction. See [ADR-0005](../adr/0005-wasapi-playback.md).

#![allow(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "see wasapi.rs's identical allow"
)]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::Select;
use crate::audio::{AudioPlayback, AudioPlaybackConfig, PlaybackError};
use mediaway_common::{AudioFrame, Bytes, CodecKind, Rational, SampleFormat, StreamInfo};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, IAudioClient, IAudioClock,
    IAudioRenderClient, IMMDeviceEnumerator, MMDeviceEnumerator, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree,
};

use crate::windows_audio::wasapi::{ComGuard, closed_audio_info, read_float_mix, resolve_endpoint};

/// Bounded queue capacity in `AudioFrame`s, mirroring `wasapi.rs`'s `PCM_QUEUE_CAP`.
/// Unlike capture (drop-oldest), a full playback queue rejects the new frame
/// (`PlaybackError::QueueFull`) instead — see facade ADR-0004.
const PLAYBACK_QUEUE_CAP: usize = 64;

/// Render-side bytes-per-sample-frame bytes-per-sample for the only format v1 accepts.
const BYTES_PER_SAMPLE_F32: usize = 4;

/// Silence kept in the render buffer while nothing is queued: a fiftieth of a second (20 ms).
/// Queued PCM is written behind whatever the buffer already holds, so this is also the most
/// silence that can sit in front of the first frame written after a quiet stretch or a
/// [`WindowsWasapiPlayback::flush`] (ADR-0005 § Updates 2026-10-13).
const SILENCE_LEAD_DIVISOR: u32 = 50;

/// How long [`WindowsWasapiPlayback::flush`] waits for the worker before giving up.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(1);

struct PlaybackQueueState {
    frames: VecDeque<AudioFrame>,
    /// Byte offset already consumed from the front frame's `data` (a frame's
    /// byte length rarely divides evenly by a render period's byte count).
    cursor: usize,
}

struct PlaybackSharedQueue {
    state: Mutex<PlaybackQueueState>,
    stop: AtomicBool,
    underrun_count: AtomicU64,
    /// Set by `pump_playback_loop` when it stops due to a real WASAPI
    /// failure, as opposed to `stop` being set by a caller-requested
    /// `close()` — see ADR-0005 § `DeviceLost` (mirrors `wasapi.rs`'s
    /// `SharedQueue::device_lost`).
    device_lost: AtomicBool,
    /// Queued PCM sample frames the endpoint has played since `open` or the last
    /// [`WindowsWasapiPlayback::flush`]; silence the worker inserted is not counted.
    played: AtomicU64,
    /// Set by [`WindowsWasapiPlayback::flush`]; the worker stops, resets and restarts the
    /// client, then clears it. While it is set the worker takes nothing from the queue.
    flush_requested: AtomicBool,
}

/// Windows WASAPI shared-mode render (playback) session.
///
/// # Zero-Copy status (CPU ⚡)
///
/// Not CPU Zero-Copy, mirroring [`crate::windows_audio::wasapi::WindowsWasapiCapture`]'s
/// documented reason in the opposite direction: `IAudioRenderClient::GetBuffer`'s
/// pointer is only valid until the matching `ReleaseBuffer`, and the render engine
/// always targets its own buffer — there is no API to hand it caller-owned memory
/// instead, so queued PCM is copied into the OS-owned render buffer once per period
/// (see `pump_playback_loop`). `AudioPlayback::write_frame` pushing into the internal
/// queue costs no additional copy: `AudioFrame::data` is `bytes::Bytes` (refcounted),
/// so queuing is a move + refcount bump, not a `memcpy`.
pub struct WindowsWasapiPlayback {
    inner: Option<PlaybackSession>,
}

struct PlaybackSession {
    stream_info: StreamInfo,
    queue: Arc<PlaybackSharedQueue>,
    worker: Option<JoinHandle<()>>,
}

impl WindowsWasapiPlayback {
    /// Open WASAPI shared-mode render playback for `config` (default render
    /// endpoint for [`Select::Default`]; see [`Select`] for the other
    /// resolution modes).
    ///
    /// # Errors
    ///
    /// Returns [`PlaybackError`] when the endpoint or mix format is unavailable.
    /// Only IEEE float mix formats are accepted (reject others — no silent mishandling).
    pub fn open(config: &AudioPlaybackConfig) -> Result<Self, PlaybackError> {
        if config.sample_format != SampleFormat::F32 {
            return Err(PlaybackError::Unsupported);
        }

        let queue = Arc::new(PlaybackSharedQueue {
            state: Mutex::new(PlaybackQueueState {
                frames: VecDeque::new(),
                cursor: 0,
            }),
            stop: AtomicBool::new(false),
            underrun_count: AtomicU64::new(0),
            device_lost: AtomicBool::new(false),
            played: AtomicU64::new(0),
            flush_requested: AtomicBool::new(false),
        });
        // clone: Arc share with WASAPI render worker thread
        let queue_worker = Arc::clone(&queue);
        // clone: `Select` must be moved into the `'static` worker thread
        // (`thread::Builder::spawn(move || ..)`), but `config` is only
        // borrowed for the duration of `open` (ADR-0005).
        let select = config.select.clone();

        let (tx_info, rx_info) = std::sync::mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("mediaway-wasapi-playback".into())
            .spawn(move || {
                let result = run_wasapi_playback_worker(&select, &queue_worker, &tx_info);
                if let Err(e) = result {
                    let _ = tx_info.send(Err(e));
                }
            })
            .map_err(|_| PlaybackError::Backend)?;

        let stream_info = rx_info.recv().map_err(|_| PlaybackError::Backend)??;

        Ok(Self {
            inner: Some(PlaybackSession {
                stream_info,
                queue,
                worker: Some(worker),
            }),
        })
    }

    /// Sample frames of queued PCM the endpoint has played since `open` or the last
    /// [`flush`](Self::flush), from `IAudioClock::GetPosition` (the position of the sample
    /// playing through the endpoint, not of the last one handed to the engine). Silence the
    /// worker inserts while the queue is empty is not counted, so this is a clock of the
    /// caller's own audio: it stands still through an underrun. The worker updates it every
    /// poll (about 5 ms).
    #[must_use]
    pub fn played_frames(&self) -> u64 {
        self.inner
            .as_ref()
            .map_or(0, |s| s.queue.played.load(Ordering::Acquire))
    }

    /// Discards everything not yet played, queued or already in the render buffer
    /// (`IAudioClient::Stop`, `Reset`, `Start`), and sets
    /// [`played_frames`](Self::played_frames) back to 0. Frames written after this returns
    /// play next. Blocks until the worker has reset the client (one poll, about 5 ms).
    ///
    /// # Errors
    ///
    /// [`PlaybackError::Closed`] after `close`, [`PlaybackError::DeviceLost`] if the device
    /// went away, [`PlaybackError::Backend`] if the worker did not answer within a second.
    pub fn flush(&mut self) -> Result<(), PlaybackError> {
        let Some(session) = self.inner.as_ref() else {
            return Err(PlaybackError::Closed);
        };
        let queue = &session.queue;
        if queue.device_lost.load(Ordering::Relaxed) {
            return Err(PlaybackError::DeviceLost);
        }
        {
            let Ok(mut state) = queue.state.lock() else {
                return Err(PlaybackError::Backend);
            };
            queue.flush_requested.store(true, Ordering::SeqCst);
            state.frames.clear();
            state.cursor = 0;
        }
        let asked = std::time::Instant::now();
        while queue.flush_requested.load(Ordering::SeqCst) {
            if queue.device_lost.load(Ordering::Relaxed) {
                return Err(PlaybackError::DeviceLost);
            }
            if asked.elapsed() > FLUSH_TIMEOUT {
                return Err(PlaybackError::Backend);
            }
            thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }
}

impl AudioPlayback for WindowsWasapiPlayback {
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

    fn write_frame(&mut self, frame: AudioFrame) -> Result<(), PlaybackError> {
        let Some(session) = self.inner.as_ref() else {
            return Err(PlaybackError::Closed);
        };
        if session.queue.device_lost.load(Ordering::Relaxed) {
            // The session is no longer usable — every subsequent write is
            // rejected the same way a `Closed` session would be, not just
            // the first one after the device disappeared.
            return Err(PlaybackError::DeviceLost);
        }
        let expected_rate = session.stream_info.sample_rate().unwrap_or(0);
        let expected_channels = session.stream_info.channels().unwrap_or(0);
        if frame.format != SampleFormat::F32
            || frame.sample_rate != expected_rate
            || frame.channels != expected_channels
        {
            return Err(PlaybackError::InvalidInput);
        }

        let Ok(mut state) = session.queue.state.lock() else {
            return Err(PlaybackError::Backend);
        };
        if state.frames.len() >= PLAYBACK_QUEUE_CAP {
            drop(state);
            return Err(PlaybackError::QueueFull(frame));
        }
        state.frames.push_back(frame);
        Ok(())
    }

    fn underrun_count(&self) -> u64 {
        self.inner
            .as_ref()
            .map_or(0, |s| s.queue.underrun_count.load(Ordering::Relaxed))
    }

    fn close(&mut self) -> Result<(), PlaybackError> {
        let Some(mut session) = self.inner.take() else {
            return Ok(());
        };
        session.queue.stop.store(true, Ordering::SeqCst);
        if let Some(h) = session.worker.take() {
            let _ = h.join();
        }
        Ok(())
    }
}

impl Drop for WindowsWasapiPlayback {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn run_wasapi_playback_worker(
    select: &Select,
    queue: &PlaybackSharedQueue,
    tx_info: &std::sync::mpsc::SyncSender<Result<StreamInfo, PlaybackError>>,
) -> Result<(), PlaybackError> {
    // SAFETY: COM init for this worker thread.
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if hr.is_err() {
        return Err(notify_err(tx_info, PlaybackError::Backend));
    }
    let _com = ComGuard;

    let (audio_client, render, sample_rate, channels, buffer_frame_count) =
        open_wasapi_render_client(select, tx_info)?;
    let clock = DeviceClock::new(&audio_client, sample_rate).map_err(|e| notify_err(tx_info, e))?;

    let info = StreamInfo::Audio {
        id: 0,
        codec: CodecKind::RawAudio,
        time_base: Rational::new(1, sample_rate.max(1)),
        extra_data: Bytes::new(),
        sample_rate,
        channels,
    };
    let _ = tx_info.send(Ok(info));

    pump_playback_loop(
        &audio_client,
        &render,
        &clock,
        &Format {
            sample_rate,
            channels,
            buffer_frame_count,
        },
        queue,
    );
    let _ = unsafe { audio_client.Stop() };
    Ok(())
}

fn open_wasapi_render_client(
    select: &Select,
    tx_info: &std::sync::mpsc::SyncSender<Result<StreamInfo, PlaybackError>>,
) -> Result<(IAudioClient, IAudioRenderClient, u32, u16, u32), PlaybackError> {
    // SAFETY: standard in-proc COM activation.
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER) }
            .map_err(|_| PlaybackError::Backend)?;

    let device = resolve_endpoint(&enumerator, eRender, select).map_err(|e| {
        notify_err(
            tx_info,
            match e {
                crate::CaptureError::AccessDenied => PlaybackError::AccessDenied,
                crate::CaptureError::Unsupported | crate::CaptureError::InvalidInput => {
                    PlaybackError::Unsupported
                }
                _ => PlaybackError::Backend,
            },
        )
    })?;

    // SAFETY: Activate turbofish constructs IAudioClient from IMMDevice.
    let audio_client: IAudioClient = unsafe { device.Activate::<IAudioClient>(CLSCTX_ALL, None) }
        .map_err(|_| PlaybackError::Backend)?;

    let format_ptr = unsafe { audio_client.GetMixFormat() }.map_err(|_| PlaybackError::Backend)?;
    // SAFETY: `format_ptr` is a live `WAVEFORMATEX*` just returned by `GetMixFormat`.
    let (sample_rate, channels, valid) = unsafe { read_float_mix(format_ptr) };
    if !valid {
        // SAFETY: `format_ptr` was allocated by `GetMixFormat` (`CoTaskMemAlloc`);
        // freeing it with `CoTaskMemFree` is the documented pairing.
        unsafe { CoTaskMemFree(Some(format_ptr.cast())) };
        return Err(notify_err(tx_info, PlaybackError::Unsupported));
    }

    // SAFETY: Initialize with the mix format from GetMixFormat, no loopback flag
    // (render mode, not capture).
    let init = unsafe {
        audio_client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 10_000_000, 0, format_ptr, None)
    };
    // SAFETY: pairs with the `GetMixFormat` allocation above.
    unsafe { CoTaskMemFree(Some(format_ptr.cast())) };
    init.map_err(|_| PlaybackError::Backend)?;

    let buffer_frame_count =
        unsafe { audio_client.GetBufferSize() }.map_err(|_| PlaybackError::Backend)?;
    let render: IAudioRenderClient =
        unsafe { audio_client.GetService() }.map_err(|_| PlaybackError::Backend)?;
    unsafe { audio_client.Start() }.map_err(|_| PlaybackError::Backend)?;
    Ok((
        audio_client,
        render,
        sample_rate,
        channels,
        buffer_frame_count,
    ))
}

/// The endpoint's stream position (`IAudioClock`), in sample frames.
struct DeviceClock {
    clock: IAudioClock,
    /// `IAudioClock::GetFrequency`: position units a second.
    frequency: u64,
    sample_rate: u32,
}

impl DeviceClock {
    fn new(audio_client: &IAudioClient, sample_rate: u32) -> Result<Self, PlaybackError> {
        // SAFETY: `GetService` on an initialized render client, the documented way to get its
        // `IAudioClock`.
        let clock: IAudioClock =
            unsafe { audio_client.GetService() }.map_err(|_| PlaybackError::Backend)?;
        // SAFETY: a plain getter on a live `IAudioClock`.
        let frequency = unsafe { clock.GetFrequency() }.map_err(|_| PlaybackError::Backend)?;
        Ok(Self {
            clock,
            frequency: frequency.max(1),
            sample_rate,
        })
    }

    /// Sample frames played since the client started or was last reset.
    fn frames(&self) -> Option<u64> {
        let mut position = 0u64;
        // SAFETY: `position` is a live out-parameter for the call; the optional QPC
        // out-parameter is not asked for.
        unsafe { self.clock.GetPosition(&raw mut position, None) }.ok()?;
        let frames =
            u128::from(position) * u128::from(self.sample_rate) / u128::from(self.frequency);
        Some(u64::try_from(frames).unwrap_or(u64::MAX))
    }
}

/// The render format the pump works in.
struct Format {
    sample_rate: u32,
    channels: u16,
    buffer_frame_count: u32,
}

/// What the worker released to the render buffer, in order, until it has been played:
/// stretches of the caller's PCM and of inserted silence, so that a stream position maps back
/// to how much of the caller's PCM has played ([`WindowsWasapiPlayback::played_frames`]).
///
/// Pure and hardware-independent, exercised directly in `wasapi_playback_tests.rs`.
#[derive(Debug, Default)]
struct PlayedTracker {
    /// Released stretches not fully played yet: start position, frames, the caller's PCM.
    released: VecDeque<(u64, u64, bool)>,
    /// The stream position after the last release.
    end: u64,
    /// The caller's PCM frames in stretches played and dropped from `released`.
    played_before: u64,
}

impl PlayedTracker {
    /// Records `frames` released after everything before: the caller's PCM, or silence.
    fn release(&mut self, frames: u64, data: bool) {
        if frames == 0 {
            return;
        }
        match self.released.back_mut() {
            Some(last) if last.2 == data => last.1 += frames,
            _ => self.released.push_back((self.end, frames, data)),
        }
        self.end += frames;
    }

    /// The caller's PCM frames played once the stream is at `position`.
    fn played(&mut self, position: u64) -> u64 {
        while let Some(&(start, frames, data)) = self.released.front() {
            if start + frames > position {
                break;
            }
            if data {
                self.played_before += frames;
            }
            self.released.pop_front();
        }
        let partial = match self.released.front() {
            Some(&(start, _, true)) => position.saturating_sub(start),
            _ => 0,
        };
        self.played_before + partial
    }
}

fn pump_playback_loop(
    audio_client: &IAudioClient,
    render: &IAudioRenderClient,
    clock: &DeviceClock,
    format: &Format,
    queue: &PlaybackSharedQueue,
) {
    let frame_size = usize::from(format.channels) * BYTES_PER_SAMPLE_F32;
    let silence_lead = (format.sample_rate / SILENCE_LEAD_DIVISOR).max(1);
    let mut tracker = PlayedTracker::default();
    while !queue.stop.load(Ordering::Relaxed) {
        if queue.flush_requested.load(Ordering::SeqCst) {
            // SAFETY: plain calls on the live client this thread owns; `Reset` is only valid
            // on a stopped stream, hence the `Stop` before it.
            let reset = unsafe { audio_client.Stop() }
                .and_then(|()| unsafe { audio_client.Reset() })
                .and_then(|()| unsafe { audio_client.Start() });
            if reset.is_err() {
                queue.device_lost.store(true, Ordering::SeqCst);
                break;
            }
            tracker = PlayedTracker::default();
            queue.played.store(0, Ordering::Release);
            queue.flush_requested.store(false, Ordering::SeqCst);
        }
        let Ok(padding) = (unsafe { audio_client.GetCurrentPadding() }) else {
            // A real WASAPI failure, not a caller-requested stop — see
            // ADR-0005 (mirrors `wasapi.rs::pump_capture_loop`).
            queue.device_lost.store(true, Ordering::SeqCst);
            break;
        };
        if let Some(position) = clock.frames() {
            queue
                .played
                .store(tracker.played(position), Ordering::Release);
        }
        let available_frames = format.buffer_frame_count.saturating_sub(padding);
        let queued_frames = if frame_size == 0 {
            0
        } else {
            queue.state.lock().map_or(0, |state| {
                let bytes: usize = state.frames.iter().map(|f| f.data.len()).sum();
                bytes.saturating_sub(state.cursor) / frame_size
            })
        };
        // The caller's PCM goes in as far as it fits. With nothing queued, silence keeps a
        // short lead in the buffer only (`SILENCE_LEAD_DIVISOR`): filling the whole buffer
        // with it would put a buffer's length of silence in front of the next frame written.
        let (frames, data) = if queued_frames > 0 {
            (
                available_frames.min(u32::try_from(queued_frames).unwrap_or(u32::MAX)),
                true,
            )
        } else {
            (
                available_frames.min(silence_lead.saturating_sub(padding)),
                false,
            )
        };
        if frames == 0 {
            thread::sleep(Duration::from_millis(5));
            continue;
        }

        // SAFETY: `frames` is at most `available_frames`, derived from `GetBufferSize`/
        // `GetCurrentPadding`, so it never exceeds the render buffer's real capacity.
        let Ok(data_ptr) = (unsafe { render.GetBuffer(frames) }) else {
            // Same real-failure reasoning as the `GetCurrentPadding` break
            // above.
            queue.device_lost.store(true, Ordering::SeqCst);
            break;
        };

        let flags = if data {
            let need_bytes = frames as usize * frame_size;
            // SAFETY: `data_ptr` is valid for `need_bytes` writes until the matching
            // `ReleaseBuffer` below; `need_bytes` is derived from `frames` and `frame_size`,
            // the packet size just requested.
            let dst = unsafe { std::slice::from_raw_parts_mut(data_ptr, need_bytes) };
            let written = queue.state.lock().map_or(0, |mut state| {
                if queue.flush_requested.load(Ordering::SeqCst) {
                    // A flush came in after the queue was measured: what is queued now was
                    // written after it and must not go into a buffer about to be reset.
                    return 0;
                }
                let PlaybackQueueState { frames, cursor } = &mut *state;
                fill_from_queue(frames, cursor, dst)
            });
            if written < need_bytes {
                // Only a flush shrinks the queue under the worker, and the buffer is about to
                // be reset; the rest is zeroed rather than left as whatever was there.
                dst[written..].fill(0);
            }
            0
        } else {
            queue.underrun_count.fetch_add(1, Ordering::Relaxed);
            AUDCLNT_BUFFERFLAGS_SILENT.0 as u32
        };
        tracker.release(u64::from(frames), data);

        // SAFETY: pairs with the `GetBuffer` call above; `frames` matches the count requested.
        let _ = unsafe { render.ReleaseBuffer(frames, flags) };
    }
}

/// Copy up to `dst.len()` bytes from the front of `queue` into `dst`, popping
/// fully-consumed frames and advancing `cursor` for a partially-consumed front
/// frame. Returns the number of bytes actually written (`< dst.len()` means the
/// queue ran dry — the caller silence-fills the remainder).
///
/// Pure and hardware-independent by construction — no WASAPI/COM types appear
/// here, so it is exercised directly in `wasapi_playback_tests.rs`.
fn fill_from_queue(queue: &mut VecDeque<AudioFrame>, cursor: &mut usize, dst: &mut [u8]) -> usize {
    let mut written = 0usize;
    while written < dst.len() {
        let Some(front) = queue.front() else {
            break;
        };
        if *cursor >= front.data.len() {
            queue.pop_front();
            *cursor = 0;
            continue;
        }
        let remaining_in_frame = front.data.len() - *cursor;
        let need = dst.len() - written;
        let take = remaining_in_frame.min(need);
        dst[written..written + take].copy_from_slice(&front.data[*cursor..*cursor + take]);
        written += take;
        *cursor += take;
        if *cursor >= front.data.len() {
            queue.pop_front();
            *cursor = 0;
        }
    }
    written
}

fn notify_err(
    tx: &std::sync::mpsc::SyncSender<Result<StreamInfo, PlaybackError>>,
    err: PlaybackError,
) -> PlaybackError {
    // clone: PlaybackError must be sent to the `open` caller via the channel and
    // also returned to the worker's caller; PlaybackError is a small value type
    // (no payload here — Unsupported/Backend/AccessDenied carry no data).
    let _ = tx.send(Err(err.clone()));
    err
}

#[cfg(test)]
#[path = "wasapi_playback_tests.rs"]
mod tests;
