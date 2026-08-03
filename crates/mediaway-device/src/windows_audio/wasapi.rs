//! WASAPI microphone / loopback / process-loopback capture.

#![allow(unsafe_code)]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::{CaptureError, Select};
use mediaway_common::{AudioFrame, Bytes, CodecKind, SampleFormat, StreamInfo};

use crate::windows_audio::wasapi_config::{
    WasapiCaptureConfig as AudioCaptureConfig, WasapiProcessTreeScope as ProcessTreeScope,
    WasapiSource as AudioCaptureSource,
};
use windows::Win32::Devices::FunctionDiscovery::{
    PKEY_Device_FriendlyName, PKEY_DeviceInterface_FriendlyName,
};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
    DEVICE_STATE_ACTIVE, EDataFlow, IAudioCaptureClient, IAudioClient, IMMDevice,
    IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX, WAVEFORMATEXTENSIBLE, eCapture,
    eConsole, eRender,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
use windows::Win32::System::Com::{
    CLSCTX_ALL, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize, STGM_READ,
};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::core::GUID;

use crate::windows_audio::wasapi_process;

const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID =
    GUID::from_u128(0x0000_0003_0000_0010_8000_00aa_0038_9b71);

const PCM_QUEUE_CAP: usize = 64;

struct SharedQueue {
    frames: Mutex<VecDeque<AudioFrame>>,
    stop: AtomicBool,
    /// Set by `pump_capture_loop` when it stops due to a real WASAPI
    /// failure (e.g. `AUDCLNT_E_DEVICE_INVALIDATED` from an unplugged
    /// endpoint), as opposed to `stop` being set by a caller-requested
    /// `close()` — see ADR-0005 § `DeviceLost`.
    device_lost: AtomicBool,
}

/// Windows WASAPI capture session (mic or render loopback).
///
/// # Zero-Copy status (CPU ⚡)
///
/// This is **not** CPU Zero-Copy. `WASAPI`'s `IAudioCaptureClient::GetBuffer` pointer is only
/// valid until the matching `ReleaseBuffer`, and shared-mode capture requires releasing each
/// packet before the next `GetBuffer` can succeed — the client cannot hold it open indefinitely.
/// `AudioCapture::poll_frame` returns an owned `AudioFrame` with no explicit release/lifetime
/// hook (unlike `VideoFrameStorage::Gpu`'s `release_frame`), so the PCM must be copied out of
/// the WASAPI-owned buffer into caller-owned memory before the worker thread releases it and
/// moves on to the next period. Borrowing the WASAPI buffer directly (e.g. a custom `Bytes`
/// vtable that defers `ReleaseBuffer` to `Drop`) would also collapse the bounded, drop-oldest
/// `PCM_QUEUE_CAP`-frame queue down to a single in-flight packet, since WASAPI disallows a
/// second `GetBuffer` until the prior one is released — a slow consumer would then risk
/// audio-engine overrun instead of graceful oldest-frame drop. That would trade a bounded,
/// well-understood backpressure model for a strictly worse one to reach the same per-period
/// byte count. One copy per period is the honest floor here (see `pump_capture_loop`).
pub struct WindowsWasapiCapture {
    inner: Option<WasapiSession>,
}

struct WasapiSession {
    stream_info: StreamInfo,
    queue: Arc<SharedQueue>,
    worker: Option<JoinHandle<()>>,
}

impl WindowsWasapiCapture {
    /// Open WASAPI capture for `config` (default endpoints when index is `0`).
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError`] when the endpoint or mix format is unavailable.
    /// Only IEEE float mix formats are accepted (reject others — no silent mishandling).
    pub fn open(config: &AudioCaptureConfig) -> Result<Self, CaptureError> {
        if config.sample_format != SampleFormat::F32 {
            return Err(CaptureError::Unsupported);
        }
        if config.time_base.den == 0 {
            return Err(CaptureError::InvalidInput);
        }

        let queue = Arc::new(SharedQueue {
            frames: Mutex::new(VecDeque::new()),
            stop: AtomicBool::new(false),
            device_lost: AtomicBool::new(false),
        });
        // clone: Arc share with WASAPI worker thread
        let queue_worker = Arc::clone(&queue);
        // clone: `AudioCaptureSource` must be moved into the `'static`
        // worker thread (`thread::Builder::spawn(move || ..)`), but
        // `config` is only borrowed for the duration of `open` — `Select`
        // is intentionally owned, not borrowed, for exactly this reason
        // (ADR-0005).
        let source = config.source.clone();
        let time_base = config.time_base;

        let (tx_info, rx_info) = std::sync::mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("mediaway-wasapi".into())
            .spawn(move || {
                let result = run_wasapi_worker(source, time_base, &queue_worker, &tx_info);
                if let Err(e) = result {
                    let _ = tx_info.send(Err(e));
                }
            })
            .map_err(|_| CaptureError::Backend)?;

        let stream_info = rx_info.recv().map_err(|_| CaptureError::Backend)??;

        Ok(Self {
            inner: Some(WasapiSession {
                stream_info,
                queue,
                worker: Some(worker),
            }),
        })
    }
}

impl WindowsWasapiCapture {
    /// See `mediaway-device-audio::AudioCapture::stream_info`/
    /// `mediaway-device-desktop::DesktopAudioCapture::stream_info` — each domain crate's
    /// wrapper type delegates here. Inherent (not a trait impl): this engine is shared by
    /// two different public traits in two different downstream crates
    /// (`mediaway-device/adr/0007-domain-crate-split.md`'s "(b)" decision), so it cannot
    /// itself implement either — the wrappers do.
    #[must_use]
    pub fn stream_info(&self) -> &StreamInfo {
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

    /// See `stream_info`'s doc for why this is inherent, not a trait method.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError`] on backend failure or device loss.
    pub fn poll_frame(&mut self) -> Result<Option<AudioFrame>, CaptureError> {
        let Some(session) = self.inner.as_ref() else {
            return Err(CaptureError::Closed);
        };
        {
            let mut q = session
                .queue
                .frames
                .lock()
                .map_err(|_| CaptureError::Backend)?;
            if let Some(frame) = q.pop_front() {
                return Ok(Some(frame));
            }
        }
        // Any frames already captured before the device was lost are drained
        // above first; only once the queue is empty does a lost device
        // surface as an error (every call, not just once — same "session is
        // no longer usable" contract as `CaptureError::Closed`).
        if session.queue.device_lost.load(Ordering::Relaxed) {
            return Err(CaptureError::DeviceLost);
        }
        Ok(None)
    }

    /// See `stream_info`'s doc for why this is inherent, not a trait method.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError`] on backend failure.
    pub fn close(&mut self) -> Result<(), CaptureError> {
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

impl Drop for WindowsWasapiCapture {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

impl WindowsWasapiCapture {
    /// Open microphone capture for `config` — the friendly, `mediaway-device-audio`-facing
    /// entry point. Translates into this engine's internal [`AudioCaptureConfig`]
    /// (`WasapiCaptureConfig`) and calls [`Self::open`]. `mediaway-device-windows-desktop`
    /// calls [`Self::open`] directly instead, with a `Loopback`/`ProcessLoopback` source —
    /// see `mediaway-device/adr/0007-domain-crate-split.md`'s "(b)" decision.
    ///
    /// # Errors
    ///
    /// See [`Self::open`].
    pub fn open_microphone(
        config: &crate::audio::AudioCaptureConfig,
    ) -> Result<Self, CaptureError> {
        Self::open(&AudioCaptureConfig {
            source: AudioCaptureSource::Microphone {
                select: config.select.clone(),
            },
            time_base: config.time_base,
            sample_format: config.sample_format,
        })
    }
}

impl crate::audio::AudioCapture for WindowsWasapiCapture {
    fn stream_info(&self) -> &StreamInfo {
        Self::stream_info(self)
    }

    fn poll_frame(&mut self) -> Result<Option<AudioFrame>, CaptureError> {
        Self::poll_frame(self)
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        Self::close(self)
    }
}

/// Only reused within this crate (`wasapi_playback.rs`) — `pub(crate)` would be enough,
/// but this module (`mod wasapi;`) is already private, so clippy's `redundant_pub_crate`
/// and rustc's `unreachable_pub` pull in opposite directions on the exact same item; the
/// documented `#[allow]` below is this file's standing resolution, same as `wasapi.rs`
/// used before this crate split.
#[allow(clippy::redundant_pub_crate)]
pub(crate) fn closed_audio_info() -> &'static StreamInfo {
    use std::sync::OnceLock;
    static INFO: OnceLock<StreamInfo> = OnceLock::new();
    INFO.get_or_init(|| StreamInfo::Audio {
        id: 0,
        codec: CodecKind::RawAudio,
        time_base: mediaway_common::Rational::new(1, 48_000),
        sample_rate: 0,
        channels: 0,
        extra_data: Bytes::new(),
    })
}

fn run_wasapi_worker(
    source: AudioCaptureSource,
    time_base: mediaway_common::Rational,
    queue: &SharedQueue,
    tx_info: &std::sync::mpsc::SyncSender<Result<StreamInfo, CaptureError>>,
) -> Result<(), CaptureError> {
    // SAFETY: COM init for this worker thread.
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if hr.is_err() {
        return Err(notify_err(tx_info, CaptureError::Backend));
    }
    let _com = ComGuard;

    let (audio_client, capture, sample_rate, channels) = open_wasapi_client(source, tx_info)?;

    let info = StreamInfo::Audio {
        id: 0,
        codec: CodecKind::RawAudio,
        time_base,
        extra_data: Bytes::new(),
        sample_rate,
        channels,
    };
    let _ = tx_info.send(Ok(info));

    pump_capture_loop(&audio_client, &capture, sample_rate, channels, queue);
    let _ = unsafe { audio_client.Stop() };
    Ok(())
}

fn open_wasapi_client(
    source: AudioCaptureSource,
    tx_info: &std::sync::mpsc::SyncSender<Result<StreamInfo, CaptureError>>,
) -> Result<(IAudioClient, IAudioCaptureClient, u32, u16), CaptureError> {
    let (data_flow, loopback, select) = match source {
        AudioCaptureSource::Microphone { select } => (eCapture, false, select),
        AudioCaptureSource::Loopback { select } => (eRender, true, select),
        AudioCaptureSource::ProcessLoopback {
            process_id,
            tree_scope,
        } => {
            let include_tree = matches!(tree_scope, ProcessTreeScope::IncludeChildren);
            return wasapi_process::open_process_loopback_client(process_id, include_tree)
                .map_err(|e| notify_err(tx_info, e));
        }
    };

    // SAFETY: standard in-proc COM activation.
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER) }
            .map_err(|_| CaptureError::Backend)?;

    let device =
        resolve_endpoint(&enumerator, data_flow, &select).map_err(|e| notify_err(tx_info, e))?;

    // SAFETY: Activate turbofish constructs IAudioClient from IMMDevice.
    let audio_client: IAudioClient = unsafe { device.Activate::<IAudioClient>(CLSCTX_ALL, None) }
        .map_err(|_| CaptureError::Backend)?;

    let format_ptr = unsafe { audio_client.GetMixFormat() }.map_err(|_| CaptureError::Backend)?;
    let (sample_rate, channels, valid) = unsafe { read_float_mix(format_ptr) };
    if !valid {
        unsafe { CoTaskMemFree(Some(format_ptr.cast())) };
        return Err(notify_err(tx_info, CaptureError::Unsupported));
    }

    let stream_flags = if loopback {
        AUDCLNT_STREAMFLAGS_LOOPBACK
    } else {
        0
    };
    // SAFETY: Initialize with mix format from GetMixFormat.
    let init = unsafe {
        audio_client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            stream_flags,
            10_000_000,
            0,
            format_ptr,
            None,
        )
    };
    unsafe { CoTaskMemFree(Some(format_ptr.cast())) };
    init.map_err(|_| CaptureError::Backend)?;

    let capture: IAudioCaptureClient =
        unsafe { audio_client.GetService() }.map_err(|_| CaptureError::Backend)?;
    unsafe { audio_client.Start() }.map_err(|_| CaptureError::Backend)?;
    Ok((audio_client, capture, sample_rate, channels))
}

fn pump_capture_loop(
    _audio_client: &IAudioClient,
    capture: &IAudioCaptureClient,
    sample_rate: u32,
    channels: u16,
    queue: &SharedQueue,
) {
    let mut pts: i64 = 0;
    while !queue.stop.load(Ordering::Relaxed) {
        let Ok(packet_length) = (unsafe { capture.GetNextPacketSize() }) else {
            // A real WASAPI failure (e.g. `AUDCLNT_E_DEVICE_INVALIDATED` on
            // unplug), not a caller-requested stop — see ADR-0005.
            queue.device_lost.store(true, Ordering::SeqCst);
            break;
        };
        if packet_length == 0 {
            thread::sleep(std::time::Duration::from_millis(5));
            continue;
        }

        let mut data_ptr: *mut u8 = std::ptr::null_mut();
        let mut num_frames = 0u32;
        let mut flags = 0u32;
        // SAFETY: WASAPI buffer pointers valid until ReleaseBuffer.
        if unsafe {
            capture.GetBuffer(
                &raw mut data_ptr,
                &raw mut num_frames,
                &raw mut flags,
                None,
                None,
            )
        }
        .is_err()
        {
            // Same real-failure reasoning as the `GetNextPacketSize` break
            // above.
            queue.device_lost.store(true, Ordering::SeqCst);
            break;
        }

        let silent = (flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0;
        if num_frames > 0 && !data_ptr.is_null() && !silent {
            let samples = num_frames as usize * channels as usize;
            let bytes = samples * 4;
            // SAFETY: `data_ptr` is valid for `bytes` reads until `ReleaseBuffer` below;
            // `bytes` is derived from `num_frames`/`channels`, the packet size WASAPI just
            // reported via `GetBuffer`.
            let pcm = unsafe { copy_pcm_buffer(data_ptr, bytes) };
            let frame = AudioFrame {
                pts,
                duration: u64::from(num_frames),
                sample_rate,
                channels,
                format: SampleFormat::F32,
                data: Bytes::from(pcm),
            };
            pts = pts.saturating_add(i64::from(num_frames));
            if let Ok(mut q) = queue.frames.lock() {
                if q.len() >= PCM_QUEUE_CAP {
                    let _ = q.pop_front();
                }
                q.push_back(frame);
            }
        }
        let _ = unsafe { capture.ReleaseBuffer(num_frames) };
    }
}

/// Copy `len` bytes out of a WASAPI-owned capture buffer into a freshly allocated,
/// owned `Vec` — the one copy the buffer-lifetime contract requires (see the Zero-Copy
/// status note on [`WindowsWasapiCapture`]).
///
/// Fills the allocation via a single `copy_nonoverlapping` instead of zero-filling it
/// first (the previous `vec![0u8; len]` path wrote the buffer twice per period — once to
/// zero it, once with the real samples — for what is still exactly one logical copy).
/// This halves the write traffic of that one copy; it does not make the path CPU ⚡.
///
/// # Safety
/// `src` must be valid for reads of `len` bytes for the duration of the call.
unsafe fn copy_pcm_buffer(src: *const u8, len: usize) -> Vec<u8> {
    let mut pcm: Vec<u8> = Vec::with_capacity(len);
    // SAFETY: `pcm` was just allocated with capacity `len`; `src` is valid for `len`
    // reads per this function's contract. Every byte of `pcm[..len]` is written by the
    // copy below before `set_len`, so no uninitialized memory is ever read or exposed.
    unsafe {
        std::ptr::copy_nonoverlapping(src, pcm.as_mut_ptr(), len);
        pcm.set_len(len);
    }
    pcm
}

fn notify_err(
    tx: &std::sync::mpsc::SyncSender<Result<StreamInfo, CaptureError>>,
    err: CaptureError,
) -> CaptureError {
    let _ = tx.send(Err(err.clone()));
    err
}

/// # Safety
/// `format_ptr` must be a valid `WAVEFORMATEX*` from `GetMixFormat`.
///
/// Reused as-is by `wasapi_playback.rs` (same crate) instead of re-deriving the same
/// IEEE-float mix-format check for render endpoints. See `closed_audio_info`'s doc for
/// why this is `pub(crate)` with an explicit `redundant_pub_crate` allow.
#[allow(clippy::redundant_pub_crate)]
pub(crate) unsafe fn read_float_mix(format_ptr: *mut WAVEFORMATEX) -> (u32, u16, bool) {
    if format_ptr.is_null() {
        return (0, 0, false);
    }
    // SAFETY: caller guarantees live mix format pointer.
    let sample_rate = unsafe { std::ptr::addr_of!((*format_ptr).nSamplesPerSec).read_unaligned() };
    let channels = unsafe { std::ptr::addr_of!((*format_ptr).nChannels).read_unaligned() };
    let tag = unsafe { std::ptr::addr_of!((*format_ptr).wFormatTag).read_unaligned() };
    let valid = if tag == WAVE_FORMAT_IEEE_FLOAT {
        true
    } else if tag == WAVE_FORMAT_EXTENSIBLE {
        let ext = format_ptr.cast::<WAVEFORMATEXTENSIBLE>();
        // SAFETY: WAVEFORMATEXTENSIBLE begins with WAVEFORMATEX when tag is EXTENSIBLE.
        let sub = unsafe { std::ptr::addr_of!((*ext).SubFormat).read_unaligned() };
        sub == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
    } else {
        false
    };
    (sample_rate, channels, valid)
}

/// Resolve `select` to a live `IMMDevice` for `data_flow` (`eCapture` /
/// `eRender`). `pub`: reused as-is by `wasapi_playback.rs` (render
/// endpoints) and `enumeration.rs` (`is_default` comparison).
///
/// # Errors
///
/// [`CaptureError::AccessDenied`] when [`Select::Default`] has no default
/// endpoint. [`CaptureError::Unsupported`] when a [`Select::Id`] wraps a
/// non-`WASAPI` [`crate::DeviceId`]. [`CaptureError::InvalidInput`]
/// when [`Select::Id`]/[`Select::NameContains`] match no active endpoint.
/// [`CaptureError::Backend`] on enumeration failures.
pub fn resolve_endpoint(
    enumerator: &IMMDeviceEnumerator,
    data_flow: EDataFlow,
    select: &Select,
) -> Result<IMMDevice, CaptureError> {
    match select {
        Select::Default => {
            // SAFETY: GetDefaultAudioEndpoint on a live enumerator.
            unsafe { enumerator.GetDefaultAudioEndpoint(data_flow, eConsole) }
                .map_err(|_| CaptureError::AccessDenied)
        }
        Select::Id(id) => {
            let endpoint_id_str = id
                .as_wasapi_endpoint_id()
                .ok_or(CaptureError::Unsupported)?;
            find_endpoint(enumerator, data_flow, |candidate| {
                endpoint_id(candidate).as_deref() == Some(endpoint_id_str)
            })
        }
        Select::NameContains(needle) => {
            let needle = needle.to_lowercase();
            find_endpoint(enumerator, data_flow, |candidate| {
                endpoint_friendly_name(candidate)
                    .is_some_and(|name| name.to_lowercase().contains(&needle))
            })
        }
    }
}

/// First active endpoint for `data_flow` matching `matches`, in
/// `EnumAudioEndpoints`' backend-defined order (not a promised stable global
/// sort — same honesty [`Select::NameContains`] documents).
fn find_endpoint(
    enumerator: &IMMDeviceEnumerator,
    data_flow: EDataFlow,
    mut matches: impl FnMut(&IMMDevice) -> bool,
) -> Result<IMMDevice, CaptureError> {
    // SAFETY: EnumAudioEndpoints borrows nothing past this call.
    let collection = unsafe { enumerator.EnumAudioEndpoints(data_flow, DEVICE_STATE_ACTIVE) }
        .map_err(|_| CaptureError::Backend)?;
    // SAFETY: GetCount is a plain out-param read.
    let count = unsafe { collection.GetCount() }.map_err(|_| CaptureError::Backend)?;
    for index in 0..count {
        // SAFETY: `index` is in `0..count` from `GetCount` above.
        let Ok(device) = (unsafe { collection.Item(index) }) else {
            continue;
        };
        if matches(&device) {
            return Ok(device);
        }
    }
    Err(CaptureError::InvalidInput)
}

/// `IMMDevice::GetId()` — the persistent endpoint ID string. `pub`:
/// reused by `enumeration.rs`.
pub fn endpoint_id(device: &IMMDevice) -> Option<String> {
    // SAFETY: GetId returns a CoTaskMemAlloc'd wide string, freed below.
    let raw = unsafe { device.GetId() }.ok()?;
    if raw.is_null() {
        return None;
    }
    // SAFETY: `raw` is a valid null-terminated wide string per `GetId`'s
    // contract, still valid at this point (freed only below).
    let id = unsafe { raw.to_string() }.ok();
    // SAFETY: matching CoTaskMemFree for the successful GetId above.
    unsafe { CoTaskMemFree(Some(raw.0.cast())) };
    id
}

/// The endpoint's display name — `IPropertyStore`/`PKEY_Device_FriendlyName`,
/// disambiguated with `PKEY_DeviceInterface_FriendlyName` when the endpoint
/// name doesn't already include it.
///
/// `pub`: reused by `crate::windows::enumeration`.
///
/// # Non-English-locale collision (real, not hypothetical)
///
/// On some non-English Windows locales, `PKEY_Device_FriendlyName` can be
/// just the generic, localized device-*class* label — e.g. Korean `마이크`
/// ("microphone") — shared verbatim by every capture endpoint of that class,
/// not a name unique to one physical device. Two different USB microphones
/// plugged into the same machine can both enumerate as `마이크`, which
/// defeats a friendly-name device picker built on this field alone. This is
/// a confirmed real-world Windows behavior, not a theoretical edge case.
/// `PKEY_DeviceInterface_FriendlyName` carries the underlying audio
/// adapter/driver's own name instead (e.g. `Realtek(R) Audio`, `USB Audio
/// Device`) — appended in parentheses when it isn't already present in the
/// endpoint name, to disambiguate without discarding the localized class
/// label.
///
/// **Confirmed on real hardware this session** (a Korean-locale Windows
/// desktop with 4 capture endpoints across 3 different adapters): the naive
/// "always append" version this replaced produced doubled suffixes like
/// `스테레오 믹스 (Realtek(R) Audio) (Realtek(R) Audio)`, because
/// `PKEY_Device_FriendlyName` here *already* embeds the driver name — this
/// crate's WASAPI stack does not reproduce the bare-generic-name collision
/// on this particular driver, but the substring check below is what makes
/// the fallback safe either way (append only when genuinely new
/// information, never duplicate).
pub fn endpoint_friendly_name(device: &IMMDevice) -> Option<String> {
    // SAFETY: OpenPropertyStore(STGM_READ) on a live endpoint device.
    let store = unsafe { device.OpenPropertyStore(STGM_READ) }.ok()?;
    let endpoint_name = property_string(&store, PKEY_Device_FriendlyName);
    let interface_name = property_string(&store, PKEY_DeviceInterface_FriendlyName);
    combine_endpoint_and_interface_names(endpoint_name, interface_name)
}

/// Pure disambiguation logic behind [`endpoint_friendly_name`] — no COM/I/O
/// — extracted so the "append only when genuinely new information" rule is
/// unit-testable without a live `IMMDevice` (same rationale as `camera.rs`'s
/// `preferred_subtype_order`).
fn combine_endpoint_and_interface_names(
    endpoint_name: Option<String>,
    interface_name: Option<String>,
) -> Option<String> {
    match (endpoint_name, interface_name) {
        (Some(endpoint_name), Some(interface_name))
            if !endpoint_name
                .to_lowercase()
                .contains(&interface_name.to_lowercase()) =>
        {
            Some(format!("{endpoint_name} ({interface_name})"))
        }
        (Some(endpoint_name), _) => Some(endpoint_name),
        (None, Some(interface_name)) => Some(interface_name),
        (None, None) => None,
    }
}

/// Read `key` off an already-opened `IPropertyStore` as a string, or `None`
/// when the property is absent/not a string type. Shared by
/// [`endpoint_friendly_name`]'s two property lookups.
fn property_string(store: &IPropertyStore, key: PROPERTYKEY) -> Option<String> {
    // SAFETY: plain property read on a store the caller already opened.
    let mut value = unsafe { store.GetValue(&raw const key) }.ok()?;
    // SAFETY: `value` is a live PROPVARIANT; PropVariantToStringAlloc
    // allocates an independent, CoTaskMemAlloc'd wide-string copy (freed
    // below) — it does not consume `value` itself.
    let raw = unsafe { PropVariantToStringAlloc(&raw const value) }.ok();
    // SAFETY: releases whatever resource `value` itself owns (e.g. the
    // property store's own string allocation), independent of the `raw`
    // copy above.
    unsafe {
        let _ = PropVariantClear(&raw mut value);
    }
    let raw = raw?;
    if raw.is_null() {
        return None;
    }
    // SAFETY: `raw` is a valid null-terminated wide string, still valid here.
    let name = unsafe { raw.to_string() }.ok();
    // SAFETY: matching CoTaskMemFree for the successful allocation above.
    unsafe { CoTaskMemFree(Some(raw.0.cast())) };
    name
}

/// RAII `CoUninitialize()` on drop for a per-call-scope `CoInitializeEx`. `pub`: reused by
/// `mediaway_device_windows`'s `capabilities`/`enumeration`/`hotplug` modules.
pub struct ComGuard;
impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

#[cfg(test)]
#[path = "wasapi_tests.rs"]
mod tests;
