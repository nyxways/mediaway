//! Windows camera capture via Media Foundation `IMFSourceReader` (CPU copy).
//!
//! [`WindowsCameraCapture::open`] enumerates video capture devices with
//! `MFEnumDeviceSources` (`MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID`), resolves
//! `CameraCaptureConfig`'s `select: Select` to an enumeration ordinal (see
//! [`resolve_camera_index`]), and negotiates NV12 or RGB32 output — preferring whichever the
//! camera already exposes natively (checked via `IMFSourceReader::GetNativeMediaType`, not
//! assumed), falling back to Media Foundation's built-in video processor conversion
//! (`MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING`) when neither is native. Most USB webcams
//! expose MJPG or YUY2 natively, not NV12/RGB32 directly.
//!
//! [`enumerate_cameras`] (used by `crate::windows::enumerate`) builds each
//! [`crate::DeviceInfo::id`] from `MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK`
//! — see [ADR-0005](../../mediaway-device/adr/0005-device-selection.md).
//!
//! # CPU-only (no DX11 Zero-Copy yet)
//!
//! This backend always copies frame bytes out of the source reader's buffer into an owned
//! [`mediaway_common::Bytes`] allocation — the same one-copy floor
//! `crate::windows_audio::WindowsWasapiCapture` documents for WASAPI:
//! `IMFMediaBuffer::Lock` / `IMF2DBuffer::Lock2D` is only valid until the matching
//! `Unlock`/`Unlock2D`, and this backend's frames are `VideoFrameStorage::Cpu` (no GPU
//! `release_frame` lifetime hook to defer the unlock through). A DX11-backed Zero-Copy path
//! (HW MFT output straight to an `ID3D11Texture2D`, matching
//! `crate::windows_desktop::WindowsScreenCapture`'s
//! [`GpuBufferHandle::DirectX11`](mediaway_common::GpuBufferHandle::DirectX11)) is a
//! follow-up, not attempted here.

#![allow(unsafe_code)]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::camera::{CameraCapture, CameraCaptureConfig, CaptureOutputPreference};
use crate::{CaptureError, DeviceId, DeviceInfo, DeviceKind, Select};
use mediaway_common::{
    Bytes, CodecKind, PixelFormat, Rational, StreamInfo, VideoFrame, VideoFrameStorage,
    VideoGeometry,
};
use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer, IMFActivate, IMFAttributes, IMFMediaBuffer, IMFMediaSource, IMFSample,
    IMFSourceReader, MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE,
    MF_MT_SUBTYPE, MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
    MF_SOURCE_READERF_ENDOFSTREAM, MF_VERSION, MFCreateAttributes, MFCreateMediaType,
    MFCreateSourceReaderFromMediaSource, MFEnumDeviceSources, MFMediaType_Video, MFSTARTUP_FULL,
    MFStartup, MFVideoFormat_NV12, MFVideoFormat_RGB32,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree};
use windows::core::{GUID, Interface, PWSTR};

/// Per-call-scope COM init guard — a local copy rather than a cross-crate dependency on
/// `mediaway-device-windows-audio` for three lines (`mediaway-device/adr/0007-domain-crate-split.md`).
struct ComGuard;
impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe {
            windows::Win32::System::Com::CoUninitialize();
        }
    }
}

/// Bounded, drop-oldest queue depth — same backpressure model as `wasapi.rs`'s
/// `PCM_QUEUE_CAP`, sized much smaller since video frames are orders of magnitude larger
/// than one audio period.
const FRAME_QUEUE_CAP: usize = 4;

/// `MF_SOURCE_READER_FIRST_VIDEO_STREAM` (`-4i32`) is a documented magic bit pattern
/// (`0xFFFFFFFC`), not a real negative stream count — the `u32` reinterpretation is exact.
#[allow(
    clippy::cast_sign_loss,
    reason = "MF_SOURCE_READER_FIRST_VIDEO_STREAM is a documented bit-pattern constant"
)]
const FIRST_VIDEO_STREAM: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

struct FrameQueue {
    frames: Mutex<VecDeque<VideoFrame>>,
    stop: AtomicBool,
}

struct CameraSession {
    stream_info: StreamInfo,
    queue: Arc<FrameQueue>,
    worker: Option<JoinHandle<()>>,
}

/// Windows camera capture session (Media Foundation `IMFSourceReader`, CPU frames).
///
/// See the module docs for the CPU-only Zero-Copy status and format negotiation.
pub struct WindowsCameraCapture {
    inner: Option<CameraSession>,
}

impl WindowsCameraCapture {
    /// Open Media Foundation camera capture for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Unsupported`] for the
    /// [`CaptureOutputPreference::ZeroCopyGpu`] preference (not implemented yet — see module
    /// docs). Returns [`CaptureError::InvalidInput`] when no camera exists at `device`'s
    /// ordinal index. Returns [`CaptureError::Backend`] on Media Foundation failures.
    pub fn open(config: &CameraCaptureConfig) -> Result<Self, CaptureError> {
        if config.output != CaptureOutputPreference::CpuFramesOk {
            return Err(CaptureError::Unsupported);
        }
        let device = resolve_camera_index(&config.select)?;

        let queue = Arc::new(FrameQueue {
            frames: Mutex::new(VecDeque::new()),
            stop: AtomicBool::new(false),
        });
        // clone: Arc share with camera worker thread
        let queue_worker = Arc::clone(&queue);
        let time_base = config.time_base;

        let (tx_info, rx_info) = std::sync::mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("mediaway-camera".into())
            .spawn(move || {
                let result = run_camera_worker(device, time_base, &queue_worker, &tx_info);
                if let Err(e) = result {
                    let _ = tx_info.send(Err(e));
                }
            })
            .map_err(|_| CaptureError::Backend)?;

        let stream_info = rx_info.recv().map_err(|_| CaptureError::Backend)??;

        Ok(Self {
            inner: Some(CameraSession {
                stream_info,
                queue,
                worker: Some(worker),
            }),
        })
    }
}

impl CameraCapture for WindowsCameraCapture {
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
        // CPU-owned frames (`VideoFrameStorage::Cpu`) hold no backend resource to release —
        // the copy already happened before the frame left the worker thread.
        Ok(())
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        let Some(mut session) = self.inner.take() else {
            return Ok(());
        };
        session.queue.stop.store(true, Ordering::SeqCst);
        // The worker's `ReadSample` call blocks until the next frame, an error, or
        // end-of-stream — `close()` can wait up to one frame interval for it to notice
        // `stop` and return, rather than cancelling mid-call (no async
        // `IMFSourceReaderCallback` pump in this CPU-copy slice).
        if let Some(h) = session.worker.take() {
            let _ = h.join();
        }
        Ok(())
    }
}

impl Drop for WindowsCameraCapture {
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
    tx_info: &std::sync::mpsc::SyncSender<Result<StreamInfo, CaptureError>>,
) -> Result<(), CaptureError> {
    // SAFETY: COM init for this worker thread (Media Foundation requires a COM apartment).
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if hr.is_err() {
        return Err(notify_err(tx_info, CaptureError::Backend));
    }
    let _com = ComGuard;
    // SAFETY: MFStartup is refcounted; matches `mediaway-decoder-windows`'s `runtime.rs`
    // convention of never calling `MFShutdown` (process lifetime).
    if unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.is_err() {
        return Err(notify_err(tx_info, CaptureError::Backend));
    }

    let activate = match activate_for_index(device_index) {
        Ok(a) => a,
        Err(e) => return Err(notify_err(tx_info, e)),
    };
    // SAFETY: `activate` is a live `IMFActivate` for a video capture device.
    let Ok(media_source) = (unsafe { activate.ActivateObject::<IMFMediaSource>() }) else {
        return Err(notify_err(tx_info, CaptureError::Backend));
    };

    let reader = match create_reader(&media_source) {
        Ok(r) => r,
        Err(e) => {
            // SAFETY: best-effort teardown of a source we failed to fully open.
            let _ = unsafe { media_source.Shutdown() };
            return Err(notify_err(tx_info, e));
        }
    };

    let (format, width, height) = match negotiate_output_type(&reader) {
        Ok(v) => v,
        Err(e) => {
            // SAFETY: best-effort teardown of a source we failed to fully open.
            let _ = unsafe { media_source.Shutdown() };
            return Err(notify_err(tx_info, e));
        }
    };

    let info = StreamInfo::Video {
        id: 0,
        codec: CodecKind::RawVideo,
        time_base,
        geometry: VideoGeometry { width, height },
        extra_data: Bytes::new(),
    };
    let _ = tx_info.send(Ok(info));

    pump_capture_loop(&reader, format, width, height, queue);

    // SAFETY: matching shutdown for the successful activation above.
    let _ = unsafe { media_source.Shutdown() };
    // SAFETY: releases the device instance; matches Microsoft's documented device-source
    // lifecycle (activate → use → `ShutdownObject`).
    let _ = unsafe { activate.ShutdownObject() };
    Ok(())
}

fn create_reader(media_source: &IMFMediaSource) -> Result<IMFSourceReader, CaptureError> {
    let mut attrs_opt: Option<IMFAttributes> = None;
    // SAFETY: out-param written on success (a fresh, empty attribute store).
    unsafe { MFCreateAttributes(&raw mut attrs_opt, 1) }.map_err(|_| CaptureError::Backend)?;
    let attrs = attrs_opt.ok_or(CaptureError::Backend)?;
    // SAFETY: plain attribute setter — enables the built-in video processor so the reader
    // can convert whatever native format the camera exposes to our requested output type.
    unsafe { attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1) }
        .map_err(|_| CaptureError::Backend)?;
    // SAFETY: `media_source` and `attrs` are both live, owned COM objects.
    unsafe { MFCreateSourceReaderFromMediaSource(media_source, &attrs) }
        .map_err(|_| CaptureError::Backend)
}

/// Enumerate `MFEnumDeviceSources`-reported video capture devices, taking ownership of every
/// slot in the returned array (the array itself is `CoTaskMemAlloc`'d by MF and freed here
/// regardless of how many/which activates the caller keeps).
fn enumerate_video_activates() -> Result<Vec<IMFActivate>, CaptureError> {
    let mut attrs_opt: Option<IMFAttributes> = None;
    // SAFETY: out-param written on success (a fresh, empty attribute store).
    unsafe { MFCreateAttributes(&raw mut attrs_opt, 1) }.map_err(|_| CaptureError::Backend)?;
    let attrs = attrs_opt.ok_or(CaptureError::Backend)?;
    // SAFETY: plain attribute setter — restricts enumeration to video capture devices.
    unsafe {
        attrs.SetGUID(
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
        )
    }
    .map_err(|_| CaptureError::Backend)?;

    let mut activates_ptr: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    // SAFETY: out-params written on success: a `CoTaskMemAlloc`'d array of `count` activate
    // objects — freed below after every slot is taken.
    unsafe { MFEnumDeviceSources(&attrs, &raw mut activates_ptr, &raw mut count) }
        .map_err(|_| CaptureError::Backend)?;

    if activates_ptr.is_null() || count == 0 {
        return Ok(Vec::new());
    }
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        // SAFETY: `activates_ptr` holds `count` valid `Option<IMFActivate>` slots from
        // `MFEnumDeviceSources`.
        if let Some(a) = unsafe { (*activates_ptr.add(i)).take() } {
            out.push(a);
        }
    }
    // SAFETY: `activates_ptr` was allocated by `MFEnumDeviceSources`; every element was
    // already taken above, and we own and free the array itself.
    unsafe {
        CoTaskMemFree(Some(activates_ptr.cast_const().cast()));
    }
    Ok(out)
}

fn activate_for_index(index: usize) -> Result<IMFActivate, CaptureError> {
    let mut activates = enumerate_video_activates()?;
    if index >= activates.len() {
        return Err(CaptureError::InvalidInput);
    }
    Ok(activates.swap_remove(index))
}

/// Resolve `select` to an `MFEnumDeviceSources` ordinal by re-enumerating
/// devices on the calling thread (cheap — no `IMFSourceReader`, no worker
/// thread). [`Select::Default`] resolves to ordinal `0` without touching
/// Media Foundation at all, matching today's behavior.
///
/// # Errors
///
/// Returns [`CaptureError::Unsupported`] when a [`Select::Id`] wraps a
/// non-Media-Foundation [`DeviceId`]. Returns [`CaptureError::InvalidInput`]
/// when [`Select::Id`]/[`Select::NameContains`] match no enumerated camera.
/// Returns [`CaptureError::Backend`] on COM/enumeration failures.
fn resolve_camera_index(select: &Select) -> Result<usize, CaptureError> {
    match select {
        Select::Default => Ok(0),
        Select::Id(id) => {
            let symlink = id
                .as_media_foundation_symbolic_link()
                .ok_or(CaptureError::Unsupported)?;
            find_camera_index(|activate| symbolic_link(activate).as_deref() == Some(symlink))
        }
        Select::NameContains(needle) => {
            let needle = needle.to_lowercase();
            find_camera_index(|activate| {
                friendly_name(activate).is_some_and(|name| name.to_lowercase().contains(&needle))
            })
        }
    }
}

/// First enumerated camera (in `MFEnumDeviceSources`' backend-defined order —
/// not a promised stable global sort) matching `matches`.
fn find_camera_index(matches: impl FnMut(&IMFActivate) -> bool) -> Result<usize, CaptureError> {
    // SAFETY: COM init for this call.
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if hr.is_err() {
        return Err(CaptureError::Backend);
    }
    let _com = ComGuard;
    let activates = enumerate_video_activates()?;
    activates
        .iter()
        .position(matches)
        .ok_or(CaptureError::InvalidInput)
}

/// Live camera enumeration for `crate::windows::enumerate` (`DeviceKind::Camera`).
///
/// `is_default` is always `false` — Media Foundation has no "default camera" concept;
/// guessing would be dishonest (ADR-0005).
///
/// # Errors
///
/// Returns [`CaptureError::Backend`] when COM initialization or device
/// enumeration fails. An empty `Vec` (not an error) means no camera is
/// attached.
pub fn enumerate_cameras() -> Result<Vec<DeviceInfo>, CaptureError> {
    // SAFETY: COM init for this call.
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if hr.is_err() {
        return Err(CaptureError::Backend);
    }
    let _com = ComGuard;
    let activates = enumerate_video_activates()?;
    Ok(activates
        .iter()
        .enumerate()
        .filter_map(|(ordinal, activate)| {
            let link = symbolic_link(activate)?;
            Some(DeviceInfo {
                id: DeviceId::from_media_foundation_symbolic_link(link),
                kind: DeviceKind::Camera,
                name: friendly_name(activate).unwrap_or_default(),
                is_default: false,
                ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
            })
        })
        .collect())
}

fn friendly_name(activate: &IMFActivate) -> Option<String> {
    allocated_string(activate, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME)
}

/// `MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK` — the persistent
/// camera symbolic link backing [`DeviceId::from_media_foundation_symbolic_link`]
/// (ADR-0005).
fn symbolic_link(activate: &IMFActivate) -> Option<String> {
    allocated_string(
        activate,
        &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
    )
}

fn allocated_string(activate: &IMFActivate, key: &windows::core::GUID) -> Option<String> {
    let mut raw = PWSTR::null();
    let mut len = 0u32;
    // SAFETY: out-params written on success; the string is `CoTaskMemAlloc`'d and freed below.
    unsafe { activate.GetAllocatedString(key, &raw mut raw, &raw mut len) }.ok()?;
    if raw.is_null() {
        return None;
    }
    // SAFETY: `raw` is a valid null-terminated wide string per `GetAllocatedString`'s
    // contract, still valid at this point (freed only below).
    let name = unsafe { raw.to_string() }.ok();
    // SAFETY: matching `CoTaskMemFree` for the successful `GetAllocatedString` above.
    unsafe {
        CoTaskMemFree(Some(raw.0.cast()));
    }
    name
}

/// Pick which of NV12/RGB32 to request first, based on what the camera's native types
/// include — pure decision logic, extracted so it is unit-testable without a live
/// `IMFSourceReader` (same rationale as `wgc.rs`'s `resized_geometry`).
fn preferred_subtype_order(natives: &[GUID]) -> [(PixelFormat, GUID); 2] {
    let rgb32_first =
        !natives.contains(&MFVideoFormat_NV12) && natives.contains(&MFVideoFormat_RGB32);
    if rgb32_first {
        [
            (PixelFormat::Bgra8, MFVideoFormat_RGB32),
            (PixelFormat::Nv12, MFVideoFormat_NV12),
        ]
    } else {
        [
            (PixelFormat::Nv12, MFVideoFormat_NV12),
            (PixelFormat::Bgra8, MFVideoFormat_RGB32),
        ]
    }
}

fn negotiate_output_type(
    reader: &IMFSourceReader,
) -> Result<(PixelFormat, u32, u32), CaptureError> {
    let natives = native_subtypes(reader);
    for (format, subtype) in preferred_subtype_order(&natives) {
        if try_set_output_type(reader, subtype).is_ok() {
            let (width, height) = current_frame_size(reader)?;
            if width == 0 || height == 0 {
                return Err(CaptureError::Backend);
            }
            return Ok((format, width, height));
        }
    }
    Err(CaptureError::Unsupported)
}

/// Real native media types this camera advertises (`IMFSourceReader::GetNativeMediaType`) —
/// the "don't assume the pixel format" check: most USB webcams expose MJPG/YUY2, not
/// NV12/RGB32, so [`negotiate_output_type`] must see what's actually there before deciding
/// whether a direct (no-conversion) match is available.
fn native_subtypes(reader: &IMFSourceReader) -> Vec<GUID> {
    let mut subtypes = Vec::new();
    for i in 0u32.. {
        // SAFETY: enumerates native media types; stops at the first failure (MF returns
        // `MF_E_NO_MORE_TYPES` once `i` exceeds the driver's advertised format count).
        let Ok(mt) = (unsafe { reader.GetNativeMediaType(FIRST_VIDEO_STREAM, i) }) else {
            break;
        };
        // SAFETY: plain attribute read on a type MF just handed back.
        if let Ok(subtype) = unsafe { mt.GetGUID(&MF_MT_SUBTYPE) } {
            subtypes.push(subtype);
        }
    }
    subtypes
}

fn try_set_output_type(reader: &IMFSourceReader, subtype: GUID) -> windows::core::Result<()> {
    // SAFETY: owned media type; plain attribute setters. No frame size is set — the reader
    // keeps the camera's native/negotiated size, read back via `current_frame_size`.
    let out_type = unsafe { MFCreateMediaType() }?;
    unsafe {
        out_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        out_type.SetGUID(&MF_MT_SUBTYPE, &raw const subtype)?;
        reader.SetCurrentMediaType(FIRST_VIDEO_STREAM, None, &out_type)
    }
}

fn current_frame_size(reader: &IMFSourceReader) -> Result<(u32, u32), CaptureError> {
    // SAFETY: reads back the type the reader just negotiated in `try_set_output_type`.
    let mt = unsafe { reader.GetCurrentMediaType(FIRST_VIDEO_STREAM) }
        .map_err(|_| CaptureError::Backend)?;
    // SAFETY: plain attribute read.
    let packed = unsafe { mt.GetUINT64(&MF_MT_FRAME_SIZE) }.map_err(|_| CaptureError::Backend)?;
    let width = u32::try_from(packed >> 32).unwrap_or(0);
    let height = u32::try_from(packed & u64::from(u32::MAX)).unwrap_or(0);
    Ok((width, height))
}

fn pump_capture_loop(
    reader: &IMFSourceReader,
    format: PixelFormat,
    width: u32,
    height: u32,
    queue: &FrameQueue,
) {
    let mut pts: i64 = 0;
    while !queue.stop.load(Ordering::Relaxed) {
        let mut stream_flags = 0u32;
        let mut sample: Option<IMFSample> = None;
        // SAFETY: `ReadSample` blocks until the next sample, an error, or end-of-stream;
        // out-params are written on success.
        let read = unsafe {
            reader.ReadSample(
                FIRST_VIDEO_STREAM,
                0,
                None,
                Some(&raw mut stream_flags),
                None,
                Some(&raw mut sample),
            )
        };
        if read.is_err() {
            break;
        }
        if stream_flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            break;
        }
        let Some(sample) = sample else {
            continue;
        };
        let Ok(data) = frame_bytes_from_sample(&sample, format, width, height) else {
            continue;
        };
        let frame = VideoFrame {
            pts,
            duration: 1,
            width,
            height,
            format,
            storage: VideoFrameStorage::Cpu { data },
        };
        pts = pts.saturating_add(1);
        if let Ok(mut q) = queue.frames.lock() {
            if q.len() >= FRAME_QUEUE_CAP {
                let _ = q.pop_front();
            }
            q.push_back(frame);
        }
    }
}

fn frame_bytes_from_sample(
    sample: &IMFSample,
    format: PixelFormat,
    width: u32,
    height: u32,
) -> Result<Bytes, CaptureError> {
    // SAFETY: `IMFSourceReader` flattens multi-buffer samples into a single buffer
    // (documented source-reader behavior), so index 0 is always the whole frame.
    let buffer = unsafe { sample.GetBufferByIndex(0) }.map_err(|_| CaptureError::Backend)?;
    match format {
        PixelFormat::Nv12 => copy_nv12(&buffer, width, height),
        PixelFormat::Bgra8 => copy_packed(&buffer, width, height, 4),
        _ => Err(CaptureError::Unsupported),
    }
}

fn copy_nv12(buffer: &IMFMediaBuffer, width: u32, height: u32) -> Result<Bytes, CaptureError> {
    if let Ok(buf2d) = buffer.cast::<IMF2DBuffer>() {
        return copy_2d_nv12(&buf2d, width, height);
    }
    copy_contiguous(buffer)
}

fn copy_2d_nv12(buf2d: &IMF2DBuffer, width: u32, height: u32) -> Result<Bytes, CaptureError> {
    let mut scanline0: *mut u8 = std::ptr::null_mut();
    let mut pitch = 0i32;
    // SAFETY: out-params written on success; buffer stays locked until `Unlock2D` below.
    unsafe { buf2d.Lock2D(&raw mut scanline0, &raw mut pitch) }
        .map_err(|_| CaptureError::Backend)?;
    if scanline0.is_null() {
        // SAFETY: matching `Unlock2D` for the successful `Lock2D` above.
        let _ = unsafe { buf2d.Unlock2D() };
        return Err(CaptureError::Backend);
    }
    let w = width as usize;
    let h = height as usize;
    let pitch_abs = pitch.unsigned_abs() as usize;
    let mut out = vec![0u8; w * h + w * (h / 2)];
    // SAFETY: `scanline0` is locked for `height` luma rows plus `height / 2` interleaved
    // chroma rows at `pitch_abs` stride (the NV12 layout the reader negotiated); each row
    // copy stays within both the source and `out`.
    unsafe {
        for row in 0..h {
            let src = scanline0.add(row * pitch_abs);
            let dst = out.as_mut_ptr().add(row * w);
            std::ptr::copy_nonoverlapping(src, dst, w);
        }
        let uv_src_base = scanline0.add(h * pitch_abs);
        let uv_dst_base = out.as_mut_ptr().add(w * h);
        for row in 0..h / 2 {
            let src = uv_src_base.add(row * pitch_abs);
            let dst = uv_dst_base.add(row * w);
            std::ptr::copy_nonoverlapping(src, dst, w);
        }
    }
    // SAFETY: matching `Unlock2D` for the successful `Lock2D` above.
    unsafe { buf2d.Unlock2D() }.map_err(|_| CaptureError::Backend)?;
    Ok(Bytes::from(out))
}

fn copy_packed(
    buffer: &IMFMediaBuffer,
    width: u32,
    height: u32,
    bytes_per_pixel: usize,
) -> Result<Bytes, CaptureError> {
    if let Ok(buf2d) = buffer.cast::<IMF2DBuffer>() {
        return copy_2d_packed(&buf2d, width, height, bytes_per_pixel);
    }
    copy_contiguous(buffer)
}

fn copy_2d_packed(
    buf2d: &IMF2DBuffer,
    width: u32,
    height: u32,
    bytes_per_pixel: usize,
) -> Result<Bytes, CaptureError> {
    let mut scanline0: *mut u8 = std::ptr::null_mut();
    let mut pitch = 0i32;
    // SAFETY: out-params written on success; buffer stays locked until `Unlock2D` below.
    unsafe { buf2d.Lock2D(&raw mut scanline0, &raw mut pitch) }
        .map_err(|_| CaptureError::Backend)?;
    if scanline0.is_null() {
        // SAFETY: matching `Unlock2D` for the successful `Lock2D` above.
        let _ = unsafe { buf2d.Unlock2D() };
        return Err(CaptureError::Backend);
    }
    let row_bytes = width as usize * bytes_per_pixel;
    let pitch_abs = pitch.unsigned_abs() as usize;
    let mut out = vec![0u8; row_bytes * height as usize];
    // SAFETY: `scanline0` is locked for `height` rows of at least `row_bytes` bytes each at
    // `pitch_abs` stride; each row copy stays within both the source and `out`.
    unsafe {
        for row in 0..height as usize {
            let src = scanline0.add(row * pitch_abs);
            let dst = out.as_mut_ptr().add(row * row_bytes);
            std::ptr::copy_nonoverlapping(src, dst, row_bytes);
        }
    }
    // SAFETY: matching `Unlock2D` for the successful `Lock2D` above.
    unsafe { buf2d.Unlock2D() }.map_err(|_| CaptureError::Backend)?;
    Ok(Bytes::from(out))
}

fn copy_contiguous(buffer: &IMFMediaBuffer) -> Result<Bytes, CaptureError> {
    let mut ptr: *mut u8 = std::ptr::null_mut();
    let mut cur_len = 0u32;
    // SAFETY: out-params written on success; buffer stays locked until `Unlock` below.
    unsafe { buffer.Lock(&raw mut ptr, None, Some(std::ptr::from_mut(&mut cur_len))) }
        .map_err(|_| CaptureError::Backend)?;
    if ptr.is_null() {
        // SAFETY: matching `Unlock` for the successful `Lock` above.
        let _ = unsafe { buffer.Unlock() };
        return Err(CaptureError::Backend);
    }
    // SAFETY: `ptr` is valid for `cur_len` bytes for the duration of the lock; copied out as
    // an owned `Vec` before `Unlock` releases it.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, cur_len as usize) }.to_vec();
    // SAFETY: matching `Unlock` for the successful `Lock` above.
    unsafe { buffer.Unlock() }.map_err(|_| CaptureError::Backend)?;
    Ok(Bytes::from(bytes))
}

fn notify_err(
    tx: &std::sync::mpsc::SyncSender<Result<StreamInfo, CaptureError>>,
    err: CaptureError,
) -> CaptureError {
    let _ = tx.send(Err(err.clone()));
    err
}

#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;
