//! Shared, refcounted DXGI Desktop Duplication sessions.
//!
//! See [ADR-0006](../adr/0006-shared-desktop-duplication.md). A dedicated driver
//! thread owns the real `IDXGIOutputDuplication` exclusively and fans frames out
//! to attached consumers via one `CopyResource` per consumer per frame — a real,
//! documented cost the exclusive (single-consumer) path in `dxgi.rs` does not pay.
//!
//! **Thread-ownership discipline**: every COM/D3D11 object this module touches
//! (`ID3D11Device`, `IDXGIOutputDuplication`, per-consumer `ID3D11Texture2D`) is
//! constructed and used *only* on the driver thread that owns it — never moved
//! across threads. This mirrors the fix `mediaway-device-ffi` ADR-0002 landed
//! for `WindowsDeviceHotplug: Send` (constructing COM objects lazily, on the
//! thread that will own them, rather than proving `Send` for a type built
//! elsewhere). The cross-thread-visible state (`ConsumerRegistry`) is
//! plain-old-data only (`u64`/`usize`/enum) — trivially `Send + Sync` with no
//! `unsafe impl` required.

#![allow(unsafe_code)]
// `dxgi_shared` is a private module (`mod dxgi_shared;`, not `pub mod`), so
// `pub(crate)` items here are only ever crate-reachable either way — same
// `redundant_pub_crate` tension `dxgi.rs` already documents for its own
// `pub(crate)` items.
#![allow(clippy::redundant_pub_crate)]

use mediaway_common::{
    Bytes, CodecKind, GpuBufferHandle, NativeHandle, PixelFormat, StreamInfo, VideoFrame,
    VideoFrameStorage, VideoGeometry,
};
use mediaway_device::{CaptureError, DeviceId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak, mpsc};
use std::thread::JoinHandle;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, ID3D11Device,
    ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    DXGI_ERROR_ACCESS_LOST, DXGI_OUTDUPL_FRAME_INFO, IDXGIDevice, IDXGIOutput1,
    IDXGIOutputDuplication, IDXGIResource,
};
use windows::core::Interface;

const POLL_TIMEOUT_MS: u32 = 16;

fn registry() -> &'static Mutex<HashMap<DeviceId, Weak<SharedDuplication>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<DeviceId, Weak<SharedDuplication>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotState {
    /// Ready to receive the next copied frame.
    Empty,
    /// A frame has been copied in and not yet handed to the consumer.
    Pending,
    /// Handed to the consumer via `poll_frame`; not yet `release_frame`d.
    Held,
}

/// Cross-thread-visible consumer bookkeeping — plain data only, no COM types,
/// so this is trivially `Send + Sync`.
struct ConsumerRecord {
    id: u64,
    /// Raw `ID3D11Texture2D*` bit pattern for this consumer's dedicated
    /// texture. Never dereferenced off the driver thread — only read back
    /// into a [`NativeHandle`] for the returned [`VideoFrame`].
    raw_texture_ptr: usize,
    state: SlotState,
}

enum ControlMsg {
    Attach {
        reply: mpsc::Sender<Result<u64, CaptureError>>,
    },
    Detach {
        id: u64,
    },
}

/// A live, shared DXGI Desktop Duplication, refcounted via `Arc`/`Weak`.
///
/// Dropping the last `Arc<SharedDuplication>` (i.e. the last attached
/// consumer closing) signals the driver thread to shut down and joins it —
/// see [`Drop`] impl.
pub(crate) struct SharedDuplication {
    consumers: Arc<Mutex<Vec<ConsumerRecord>>>,
    control_tx: mpsc::Sender<ControlMsg>,
    shutdown: Arc<AtomicBool>,
    driver_thread: Mutex<Option<JoinHandle<()>>>,
    stream_info: StreamInfo,
    /// `ID3D11Device*` bit pattern of the device that opened this shared
    /// session — a second attacher must present the *same device instance*
    /// (checked before ever sending a control message), since `CopyResource`
    /// across distinct `ID3D11Device`s is undefined without
    /// `OpenSharedResource` (deferred, see ADR-0006 § Alternatives).
    device_raw: usize,
}

impl Drop for SharedDuplication {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let handle = self
            .driver_thread
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }
}

/// Open (creating or attaching to) a shared duplication for `key`, and attach
/// as a new consumer.
///
/// # Errors
///
/// Returns [`CaptureError::InvalidInput`] if an existing shared session for
/// `key` was opened against a different `ID3D11Device` instance. Otherwise
/// propagates the underlying `DuplicateOutput`/enumeration errors on first
/// creation.
pub(crate) fn attach(
    key: DeviceId,
    device_raw: usize,
    output_index: u32,
) -> Result<(Arc<SharedDuplication>, u64, StreamInfo), CaptureError> {
    let mut map = registry().lock().unwrap_or_else(PoisonError::into_inner);

    let existing = map.get(&key).and_then(Weak::upgrade);
    let shared = if let Some(shared) = existing {
        shared
    } else {
        let shared = spawn_driver(device_raw, output_index)?;
        map.insert(key, Arc::downgrade(&shared));
        shared
    };
    // Registry lock held across the attach handshake below is intentional:
    // this is a session-open-frequency operation, not per-frame, and holding
    // it prevents a second concurrent `attach` for the same key from racing
    // the first's driver-thread spawn.

    if shared.device_raw != device_raw {
        return Err(CaptureError::InvalidInput);
    }

    let (reply_tx, reply_rx) = mpsc::channel();
    shared
        .control_tx
        .send(ControlMsg::Attach { reply: reply_tx })
        .map_err(|_| CaptureError::Backend)?;
    let consumer_id = reply_rx.recv().map_err(|_| CaptureError::Backend)??;

    let stream_info = shared.stream_info.clone();
    drop(map);
    Ok((shared, consumer_id, stream_info))
}

fn spawn_driver(
    device_raw: usize,
    output_index: u32,
) -> Result<Arc<SharedDuplication>, CaptureError> {
    let consumers: Arc<Mutex<Vec<ConsumerRecord>>> = Arc::new(Mutex::new(Vec::new()));
    let shutdown = Arc::new(AtomicBool::new(false));
    let (control_tx, control_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<StreamInfo, CaptureError>>();

    let thread_consumers = Arc::clone(&consumers);
    let thread_shutdown = Arc::clone(&shutdown);
    let handle = std::thread::Builder::new()
        .name("mediaway-dxgi-shared".to_owned())
        .spawn(move || {
            driver_loop(
                device_raw,
                output_index,
                &thread_consumers,
                &thread_shutdown,
                &control_rx,
                &ready_tx,
            );
        })
        .map_err(|_| CaptureError::Backend)?;

    let stream_info = match ready_rx.recv() {
        Ok(Ok(info)) => info,
        Ok(Err(e)) => {
            let _ = handle.join();
            return Err(e);
        }
        Err(_) => {
            let _ = handle.join();
            return Err(CaptureError::Backend);
        }
    };

    Ok(Arc::new(SharedDuplication {
        consumers,
        control_tx,
        shutdown,
        driver_thread: Mutex::new(Some(handle)),
        stream_info,
        device_raw,
    }))
}

/// Runs entirely on its own thread. Every COM object created here
/// (`device`, `duplication`, per-consumer `ID3D11Texture2D`) lives and dies
/// on this thread only — never sent elsewhere.
fn driver_loop(
    device_raw: usize,
    output_index: u32,
    consumers: &Arc<Mutex<Vec<ConsumerRecord>>>,
    shutdown: &Arc<AtomicBool>,
    control_rx: &mpsc::Receiver<ControlMsg>,
    ready_tx: &mpsc::Sender<Result<StreamInfo, CaptureError>>,
) {
    let opened = open_duplication(device_raw, output_index);
    let (device, duplication, stream_info) = match opened {
        Ok(parts) => parts,
        Err(e) => {
            let _ = ready_tx.send(Err(e));
            return;
        }
    };
    // clone: sent to the opener over the ready channel while also kept here
    // for every subsequent `attach_consumer` call's geometry lookup.
    if ready_tx.send(Ok(stream_info.clone())).is_err() {
        // Opener already gave up (e.g. panicked) — nothing left to serve.
        return;
    }

    // Driver-thread-only: the actual COM texture objects. Never exposed
    // through `consumers` (which only ever holds the raw pointer bit
    // pattern), so this map itself never needs to be `Send`.
    let mut textures: HashMap<u64, ID3D11Texture2D> = HashMap::new();
    let mut next_id: u64 = 0;

    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        while let Ok(msg) = control_rx.try_recv() {
            match msg {
                ControlMsg::Attach { reply } => {
                    let result = attach_consumer(
                        &device,
                        &stream_info,
                        &mut next_id,
                        &mut textures,
                        consumers,
                    );
                    let _ = reply.send(result);
                }
                ControlMsg::Detach { id } => {
                    textures.remove(&id);
                    let mut guard = consumers.lock().unwrap_or_else(PoisonError::into_inner);
                    guard.retain(|c| c.id != id);
                }
            }
        }

        let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut desktop_resource: Option<IDXGIResource> = None;
        // SAFETY: same AcquireNextFrame contract as the exclusive path in
        // `dxgi.rs` — this thread is the sole caller against `duplication`.
        let acquire = unsafe {
            duplication.AcquireNextFrame(
                POLL_TIMEOUT_MS,
                &raw mut frame_info,
                &raw mut desktop_resource,
            )
        };
        if let Err(e) = acquire {
            if e.code() == DXGI_ERROR_ACCESS_LOST {
                break; // Session invalidated (display change, etc.) — stop serving.
            }
            // DXGI_ERROR_WAIT_TIMEOUT (no new frame this tick) or any other
            // transient error: loop again, re-checking shutdown/control.
            continue;
        }

        let Some(desktop_resource) = desktop_resource else {
            continue;
        };
        let Ok(source_texture) = desktop_resource.cast::<ID3D11Texture2D>() else {
            // SAFETY: release the frame we failed to cast before retrying.
            let _ = unsafe { duplication.ReleaseFrame() };
            continue;
        };

        copy_to_ready_consumers(&device, &source_texture, consumers, &textures);

        // SAFETY: ReleaseFrame pairs with the AcquireNextFrame above — this
        // thread releases immediately after copying out to every ready
        // consumer, never holding the DXGI-owned frame across loop
        // iterations (unlike the per-consumer `Held` state, which only
        // gates each consumer's *own* dedicated texture).
        let _ = unsafe { duplication.ReleaseFrame() };
    }
}

/// # Safety-relevant contract
///
/// Called only from `driver_loop`, on the driver thread — `device` never
/// leaves this thread.
fn open_duplication(
    device_raw: usize,
    output_index: u32,
) -> Result<(ID3D11Device, IDXGIOutputDuplication, StreamInfo), CaptureError> {
    let raw = device_raw as *mut std::ffi::c_void;
    // SAFETY: caller (spawn_driver) guarantees a live `ID3D11Device*` for the
    // session's lifetime; reconstructed here, on the thread that will own
    // every subsequent call against it, not moved in from elsewhere.
    let device_ref =
        unsafe { ID3D11Device::from_raw_borrowed(&raw) }.ok_or(CaptureError::InvalidInput)?;
    // clone: COM AddRef for this driver thread's own owned device handle
    let device: ID3D11Device = device_ref.clone();

    let dxgi_device: IDXGIDevice = device.cast().map_err(|_| CaptureError::Backend)?;
    // SAFETY: GetAdapter is a proven, compiling precedent (see `dxgi.rs::open`).
    let adapter = unsafe { dxgi_device.GetAdapter() }.map_err(|_| CaptureError::Backend)?;
    // SAFETY: EnumOutputs is a DXGI adapter query with no retained pointers.
    let output =
        unsafe { adapter.EnumOutputs(output_index) }.map_err(|_| CaptureError::InvalidInput)?;
    let output1: IDXGIOutput1 = output.cast().map_err(|_| CaptureError::Backend)?;
    // SAFETY: DuplicateOutput on this thread's own device — this call is the
    // one this whole module exists to make succeed a second time in-process.
    let duplication =
        unsafe { output1.DuplicateOutput(&device) }.map_err(|_| CaptureError::AccessDenied)?;

    // SAFETY: GetDesc reads a fixed-size struct with no retained pointers.
    let dup_desc = unsafe { duplication.GetDesc() };
    let width = dup_desc.ModeDesc.Width;
    let height = dup_desc.ModeDesc.Height;
    if width == 0 || height == 0 {
        return Err(CaptureError::Backend);
    }

    let stream_info = StreamInfo::Video {
        id: 0,
        codec: CodecKind::RawVideo,
        // clone: `time_base` is a `Copy` `Rational` field read via `Default`
        // below in `mediaway_common::Rational::new` — no clone actually
        // needed; kept `time_base` construction explicit per-caller instead.
        time_base: mediaway_common::Rational::new(1, 60),
        geometry: VideoGeometry { width, height },
        extra_data: Bytes::new(),
    };

    Ok((device, duplication, stream_info))
}

fn attach_consumer(
    device: &ID3D11Device,
    stream_info: &StreamInfo,
    next_id: &mut u64,
    textures: &mut HashMap<u64, ID3D11Texture2D>,
    consumers: &Arc<Mutex<Vec<ConsumerRecord>>>,
) -> Result<u64, CaptureError> {
    let geometry = stream_info.geometry().unwrap_or(VideoGeometry {
        width: 0,
        height: 0,
    });
    let desc = D3D11_TEXTURE2D_DESC {
        Width: geometry.width,
        Height: geometry.height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        MiscFlags: 0,
        CPUAccessFlags: 0,
    };
    let mut texture: Option<ID3D11Texture2D> = None;
    // SAFETY: CreateTexture2D with no initial data — first content arrives
    // via the copy loop, not here. `ID3D11Device` methods (unlike the
    // immediate context) are documented thread-safe for resource creation,
    // but this call still only ever happens on the driver thread here by
    // construction, matching this module's stricter single-thread-touches-COM
    // discipline.
    unsafe {
        device
            .CreateTexture2D(&raw const desc, None, Some(&raw mut texture))
            .map_err(|_| CaptureError::Backend)?;
    }
    let texture = texture.ok_or(CaptureError::Backend)?;
    let raw_texture_ptr = Interface::as_raw(&texture) as usize;

    let id = *next_id;
    *next_id += 1;
    textures.insert(id, texture);
    consumers
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(ConsumerRecord {
            id,
            raw_texture_ptr,
            state: SlotState::Empty,
        });
    Ok(id)
}

fn copy_to_ready_consumers(
    device: &ID3D11Device,
    source: &ID3D11Texture2D,
    consumers: &Arc<Mutex<Vec<ConsumerRecord>>>,
    textures: &HashMap<u64, ID3D11Texture2D>,
) {
    // SAFETY: GetImmediateContext is a simple accessor on a live device,
    // called on the same thread that owns `device` throughout.
    let Ok(context) = (unsafe { device.GetImmediateContext() }) else {
        return;
    };
    let mut guard = consumers.lock().unwrap_or_else(PoisonError::into_inner);
    for record in guard.iter_mut() {
        // Slow-consumer policy (ADR-0006): skip, don't overwrite a
        // not-yet-consumed `Pending` frame, and never touch a `Held` texture
        // a consumer may still be reading via its GPU handle.
        if record.state != SlotState::Empty {
            continue;
        }
        let Some(dest) = textures.get(&record.id) else {
            continue;
        };
        // SAFETY: full-resource copy, same device, both textures created
        // with matching dimensions/format — `dest` is this consumer's own
        // dedicated texture, never shared with another consumer.
        unsafe { context.CopyResource(dest, source) };
        record.state = SlotState::Pending;
    }
}

/// Detach `consumer_id` from `shared` — best-effort; the driver thread
/// processes it on its next loop iteration.
pub(crate) fn detach(shared: &SharedDuplication, consumer_id: u64) {
    let _ = shared
        .control_tx
        .send(ControlMsg::Detach { id: consumer_id });
}

/// Poll the shared consumer's pending frame, mirroring
/// `mediaway_device_desktop::DesktopVideoCapture::poll_frame`'s contract.
pub(crate) fn poll_shared_frame(
    shared: &SharedDuplication,
    consumer_id: u64,
    next_pts: &mut i64,
) -> Result<Option<VideoFrame>, CaptureError> {
    let mut guard = shared
        .consumers
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let record = guard
        .iter_mut()
        .find(|c| c.id == consumer_id)
        .ok_or(CaptureError::Closed)?;
    let raw_texture_ptr = match record.state {
        SlotState::Held => return Err(CaptureError::Backend),
        SlotState::Empty => return Ok(None),
        SlotState::Pending => {
            record.state = SlotState::Held;
            record.raw_texture_ptr
        }
    };
    drop(guard);

    let geometry = shared.stream_info.geometry().unwrap_or(VideoGeometry {
        width: 0,
        height: 0,
    });
    let texture_handle = NativeHandle::new(raw_texture_ptr).ok_or(CaptureError::Backend)?;
    let pts = *next_pts;
    *next_pts += 1;
    Ok(Some(VideoFrame {
        pts,
        duration: 1,
        width: geometry.width,
        height: geometry.height,
        format: PixelFormat::Bgra8,
        storage: VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 {
            texture: texture_handle,
            subresource: 0,
        }),
    }))
}

/// Release the shared consumer's held frame, mirroring
/// `mediaway_device_desktop::DesktopVideoCapture::release_frame`'s contract.
pub(crate) fn release_shared_frame(
    shared: &SharedDuplication,
    consumer_id: u64,
) -> Result<(), CaptureError> {
    let mut guard = shared
        .consumers
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let record = guard
        .iter_mut()
        .find(|c| c.id == consumer_id)
        .ok_or(CaptureError::Closed)?;
    if record.state == SlotState::Held {
        record.state = SlotState::Empty;
    }
    drop(guard);
    Ok(())
}

#[cfg(test)]
#[path = "dxgi_shared_tests.rs"]
mod tests;
