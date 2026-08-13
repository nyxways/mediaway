//! Shared, refcounted DXGI Desktop Duplication sessions.
//!
//! See [ADR-0006](../adr/windows/0006-shared-desktop-duplication.md) (shared driver
//! thread, registry keying) and [ADR-0007](../adr/windows/0007-ring-buffer-shared-desktop-duplication.md)
//! (ring-buffered fan-out this module implements). A dedicated driver thread owns the
//! real `IDXGIOutputDuplication` exclusively and publishes each frame into a
//! fixed-depth ring of GPU textures; any number of caught-up consumers share a
//! published frame via a cheap `Arc` clone — a real Zero-Copy fan-out, not a
//! per-consumer `CopyResource`. The one remaining mandatory copy is
//! `AcquireNextFrame` → ring slot (required: the DDA-owned resource becomes invalid
//! the instant `ReleaseFrame` runs).
//!
//! **Thread-ownership discipline**: every COM/D3D11 object this module touches
//! (`ID3D11Device`, `IDXGIOutputDuplication`, ring/transient `ID3D11Texture2D`) is
//! constructed and used *only* on the driver thread that owns it — never moved
//! across threads. This mirrors the fix `mediaway-device-ffi` ADR-0002 landed
//! for `WindowsDeviceHotplug: Send` (constructing COM objects lazily, on the
//! thread that will own them, rather than proving `Send` for a type built
//! elsewhere). The cross-thread-visible state (`ConsumerRegistry`, [`Ring`],
//! [`RingSlot`]) is plain-old-data only (`u64`/`usize`/`AtomicU64`/`Arc`) —
//! trivially `Send + Sync` with no `unsafe impl` required anywhere in this module.

#![allow(unsafe_code)]
// `dxgi_shared` is a private module (`mod dxgi_shared;`, not `pub mod`), so
// `pub(crate)` items here are only ever crate-reachable either way — same
// `redundant_pub_crate` tension `dxgi.rs` already documents for its own
// `pub(crate)` items.
#![allow(clippy::redundant_pub_crate)]

use crate::{CaptureError, DeviceId};
use mediaway_common::{
    Bytes, CodecKind, GpuBufferHandle, NativeHandle, PixelFormat, StreamInfo, VideoFrame,
    VideoFrameStorage, VideoGeometry,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak, mpsc};
use std::thread::JoinHandle;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, ID3D11Device,
    ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    DXGI_ERROR_ACCESS_LOST, DXGI_OUTDUPL_FRAME_INFO, IDXGIDevice, IDXGIOutput1,
    IDXGIOutputDuplication, IDXGIResource,
};
use windows::core::Interface;

const POLL_TIMEOUT_MS: u32 = 16;

/// Ring depth — a throughput/latency tuning knob, not a correctness bound (see
/// [ADR-0007](../adr/windows/0007-ring-buffer-shared-desktop-duplication.md) § Q1):
/// correctness (no torn reads, driver never blocks) holds at any depth ≥ 1 because
/// of the transient-publish fallback in [`publish_tick`]. 3 gives headroom for a
/// consumer viewing the previous frame while the driver just produced a new one,
/// plus one slot of slack.
const RING_DEPTH: usize = 3;

fn registry() -> &'static Mutex<HashMap<DeviceId, Weak<SharedDuplication>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<DeviceId, Weak<SharedDuplication>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// One ring slot's cross-thread-visible identity — plain data only (a raw pointer
/// bit pattern + an atomic counter), never the actual COM object, so this is
/// trivially `Send + Sync` with no `unsafe impl`.
///
/// Reclaimable by the driver once `Arc::strong_count(&self) == 1` for one of the
/// `RING_DEPTH` fixed slots (only the ring's own `Ring::slots` entry holds it — see
/// [`publish_tick`]). A transient slot (Q1 fallback) is never a member of
/// `Ring::slots`; the driver instead tracks its own extra clone in
/// [`DriverRing::transient`] and drops it (releasing the backing texture, see
/// [`reclaim_transient`]) once nothing outside that tracking clone references it.
struct RingSlot {
    /// Raw `ID3D11Texture2D*` bit pattern. Never dereferenced off the driver
    /// thread — only read back into a [`NativeHandle`] for the returned
    /// [`VideoFrame`], same discipline `ConsumerRecord` used before this ADR.
    raw_texture_ptr: usize,
    /// Bumped by the driver each time this slot's content is refreshed. `0`
    /// means "never published" — lets a consumer distinguish "nothing new
    /// since my last poll" from "nothing has ever been published" without an
    /// extra flag.
    generation: AtomicU64,
}

/// The published-frame ring, shared between the driver thread (writer) and every
/// attached consumer's calling thread (reader via [`poll_shared_frame`]).
struct Ring {
    /// Fixed-depth, allocated once at driver startup — never resized per-tick.
    slots: Vec<Arc<RingSlot>>,
    /// Currently-published slot; consumers `Arc::clone` this on `poll_frame`.
    latest: Mutex<Arc<RingSlot>>,
}

/// Cross-thread-visible consumer bookkeeping — plain data + one `Arc`, no COM
/// types, so this is trivially `Send + Sync`.
struct ConsumerRecord {
    id: u64,
    /// Generation last handed to this consumer (0 = never polled), so a
    /// caught-up consumer's `poll_frame` is a cheap "nothing new" check
    /// without touching the ring.
    last_seen_generation: u64,
    /// `Some` between `poll_frame` and `release_frame` — the consumer's own
    /// strong reference. Its `Drop` (in `release_shared_frame`) *is* the
    /// recycle signal (ADR-0007 § Q2): once no consumer holds a slot and it
    /// is no longer `Ring::latest`, the driver is free to overwrite it.
    held: Option<Arc<RingSlot>>,
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
    ring: Arc<Ring>,
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
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(StreamInfo, Arc<Ring>), CaptureError>>();

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

    let (stream_info, ring) = match ready_rx.recv() {
        Ok(Ok(parts)) => parts,
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
        ring,
        control_tx,
        shutdown,
        driver_thread: Mutex::new(Some(handle)),
        stream_info,
        device_raw,
    }))
}

/// Runs entirely on its own thread. Every COM object created here
/// (`device`, `duplication`, ring/transient `ID3D11Texture2D`s) lives and
/// dies on this thread only — never sent elsewhere.
fn driver_loop(
    device_raw: usize,
    output_index: u32,
    consumers: &Arc<Mutex<Vec<ConsumerRecord>>>,
    shutdown: &Arc<AtomicBool>,
    control_rx: &mpsc::Receiver<ControlMsg>,
    ready_tx: &mpsc::Sender<Result<(StreamInfo, Arc<Ring>), CaptureError>>,
) {
    let opened = open_duplication(device_raw, output_index);
    let (device, duplication, stream_info) = match opened {
        Ok(parts) => parts,
        Err(e) => {
            let _ = ready_tx.send(Err(e));
            return;
        }
    };

    let desc = texture_desc(&stream_info);
    let ring_init = create_ring(&device, &desc);
    let (ring, ring_textures) = match ring_init {
        Ok(parts) => parts,
        Err(e) => {
            let _ = ready_tx.send(Err(e));
            return;
        }
    };
    let ring = Arc::new(ring);

    // clone: `ring` is kept here for this thread's own per-tick publishing
    // while a second strong ref is sent to the opener for `SharedDuplication`.
    if ready_tx.send(Ok((stream_info, Arc::clone(&ring)))).is_err() {
        // Opener already gave up (e.g. panicked) — nothing left to serve.
        return;
    }

    let mut next_id: u64 = 0;
    // Driver-thread-only tracking of transient (Q1 fallback) slots: each
    // entry is the driver's own extra `Arc<RingSlot>` clone (so its
    // `strong_count` reveals whether any consumer still references it) paired
    // with the backing COM texture, reclaimed together in `reclaim_transient`.
    let mut transient: Vec<(Arc<RingSlot>, ID3D11Texture2D)> = Vec::new();

    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        while let Ok(msg) = control_rx.try_recv() {
            match msg {
                ControlMsg::Attach { reply } => {
                    let id = attach_consumer(&mut next_id, consumers);
                    let _ = reply.send(Ok(id));
                }
                ControlMsg::Detach { id } => {
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

        // SAFETY: GetImmediateContext is a simple accessor on a live device,
        // called on the same thread that owns `device` throughout.
        if let Ok(context) = unsafe { device.GetImmediateContext() } {
            reclaim_transient(&mut transient);
            publish_tick(
                &device,
                &context,
                &source_texture,
                &ring,
                &ring_textures,
                &mut transient,
                &desc,
            );
        }

        // SAFETY: ReleaseFrame pairs with the AcquireNextFrame above — this
        // thread releases immediately after copying into the ring, never
        // holding the DXGI-owned frame across loop iterations.
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
        time_base: mediaway_common::Rational::new(1, 60),
        geometry: VideoGeometry { width, height },
        extra_data: Bytes::new(),
    };

    Ok((device, duplication, stream_info))
}

/// Shared `ID3D11Texture2D` description for every ring/transient slot —
/// BGRA8, GPU-resident, shader-readable (matches what `dx11.rs`'s Zero-Copy
/// WMF wrap expects on the encode side).
fn texture_desc(stream_info: &StreamInfo) -> D3D11_TEXTURE2D_DESC {
    let geometry = stream_info.geometry().unwrap_or(VideoGeometry {
        width: 0,
        height: 0,
    });
    D3D11_TEXTURE2D_DESC {
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
    }
}

/// Create one GPU-resident `ID3D11Texture2D` matching `desc` — no initial
/// data; content arrives via `CopyResource` in `publish_tick`.
fn create_texture(
    device: &ID3D11Device,
    desc: &D3D11_TEXTURE2D_DESC,
) -> Result<ID3D11Texture2D, CaptureError> {
    let mut texture: Option<ID3D11Texture2D> = None;
    // SAFETY: `ID3D11Device` methods (unlike the immediate context) are
    // documented thread-safe for resource creation, but this call still only
    // ever happens on the driver thread here by construction, matching this
    // module's stricter single-thread-touches-COM discipline.
    unsafe {
        device
            .CreateTexture2D(&raw const *desc, None, Some(&raw mut texture))
            .map_err(|_| CaptureError::Backend)?;
    }
    texture.ok_or(CaptureError::Backend)
}

/// Allocate the `RING_DEPTH` fixed ring slots once, at driver startup.
/// Returns the cross-thread-visible [`Ring`] plus the driver-thread-only
/// backing textures, index-aligned with `Ring::slots`.
fn create_ring(
    device: &ID3D11Device,
    desc: &D3D11_TEXTURE2D_DESC,
) -> Result<(Ring, Vec<ID3D11Texture2D>), CaptureError> {
    let mut slots = Vec::with_capacity(RING_DEPTH);
    let mut textures = Vec::with_capacity(RING_DEPTH);
    for _ in 0..RING_DEPTH {
        let texture = create_texture(device, desc)?;
        let raw_texture_ptr = Interface::as_raw(&texture) as usize;
        slots.push(Arc::new(RingSlot {
            raw_texture_ptr,
            generation: AtomicU64::new(0),
        }));
        textures.push(texture);
    }
    // First slot is the initial `latest` (generation 0 == "never published" —
    // `poll_shared_frame` treats that as "no frame yet", not a real frame).
    let latest = Arc::clone(&slots[0]);
    Ok((
        Ring {
            slots,
            latest: Mutex::new(latest),
        },
        textures,
    ))
}

fn attach_consumer(next_id: &mut u64, consumers: &Arc<Mutex<Vec<ConsumerRecord>>>) -> u64 {
    let id = *next_id;
    *next_id += 1;
    consumers
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(ConsumerRecord {
            id,
            last_seen_generation: 0,
            held: None,
        });
    id
}

/// Publish `source`'s content into the ring for this tick: find a fixed slot
/// nothing outside the ring's own bookkeeping references
/// (`Arc::strong_count == 1`, ADR-0007 § Q1), copy into it, bump its
/// generation, and republish it as `latest`. If every fixed slot is currently
/// referenced (a straggling consumer or two), fall back to a one-off
/// transient slot instead of blocking or touching a slot a consumer is
/// mid-read on.
fn publish_tick(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    source: &ID3D11Texture2D,
    ring: &Ring,
    ring_textures: &[ID3D11Texture2D],
    transient: &mut Vec<(Arc<RingSlot>, ID3D11Texture2D)>,
    desc: &D3D11_TEXTURE2D_DESC,
) {
    let free = ring
        .slots
        .iter()
        .position(|slot| Arc::strong_count(slot) == 1);

    if let Some(index) = free {
        let Some(dest_texture) = ring_textures.get(index) else {
            return;
        };
        // SAFETY: full-resource copy, same device, both textures created
        // with matching dimensions/format; `dest_texture` is a fixed ring
        // slot this driver thread exclusively owns.
        unsafe { context.CopyResource(dest_texture, source) };
        let slot = &ring.slots[index];
        slot.generation.fetch_add(1, Ordering::AcqRel);
        *ring.latest.lock().unwrap_or_else(PoisonError::into_inner) = Arc::clone(slot);
        return;
    }

    // Q1 fallback: every fixed slot is referenced. Allocate a one-off
    // transient texture rather than touching a slot a consumer is mid-read
    // on or growing the ring permanently.
    let Ok(texture) = create_texture(device, desc) else {
        return; // Best-effort: skip this tick's publish on allocation failure.
    };
    // SAFETY: same reasoning as the fixed-slot copy above.
    unsafe { context.CopyResource(&texture, source) };
    let raw_texture_ptr = Interface::as_raw(&texture) as usize;
    let slot = Arc::new(RingSlot {
        raw_texture_ptr,
        generation: AtomicU64::new(1),
    });
    *ring.latest.lock().unwrap_or_else(PoisonError::into_inner) = Arc::clone(&slot);
    // The driver's own tracking clone (`slot`) plus `ring.latest`'s clone
    // above are the baseline refcount, mirroring a fixed slot's
    // array-entry-plus-latest baseline — see `reclaim_transient`.
    transient.push((slot, texture));
}

/// Drop this driver thread's tracking reference (and, once no consumer clone
/// remains either, the backing COM texture) for any transient slot no longer
/// referenced by anything beyond the driver's own tracking clone.
///
/// A transient entry's baseline is 2 strong refs (the driver's tracking
/// clone here + `Ring::latest`, or a consumer's `held` clone once one polls
/// it) — once superseded as `latest` and released by every consumer,
/// `strong_count` drops to 1 (only this function's own tracking clone), at
/// which point it is safe to drop both the `Arc` and the COM texture
/// together, on this driver thread.
fn reclaim_transient(transient: &mut Vec<(Arc<RingSlot>, ID3D11Texture2D)>) {
    transient.retain(|(slot, _texture)| Arc::strong_count(slot) > 1);
}

/// Detach `consumer_id` from `shared` — best-effort; the driver thread
/// processes it on its next loop iteration.
pub(crate) fn detach(shared: &SharedDuplication, consumer_id: u64) {
    let _ = shared
        .control_tx
        .send(ControlMsg::Detach { id: consumer_id });
}

/// Poll the shared consumer's pending frame, mirroring
/// `crate::desktop::DesktopVideoCapture::poll_frame`'s contract.
///
/// # Caveat
///
/// A caller must finish **issuing** any GPU work that reads the returned
/// frame's texture (e.g. handing it to WMF's Zero-Copy encode input) before
/// calling [`release_shared_frame`] — issue-then-drop, never
/// drop-then-issue-later. The recycle signal is this handle's `Arc` drop, not
/// a GPU completion fence (see
/// [ADR-0007](../adr/windows/0007-ring-buffer-shared-desktop-duplication.md) § Q2);
/// safety rests on D3D11 immediate-context command-submission ordering on the
/// shared device, the same trust model the existing WMF Zero-Copy path
/// already relies on and has hardware-verified.
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
    if record.held.is_some() {
        return Err(CaptureError::Backend);
    }

    let latest = Arc::clone(
        &shared
            .ring
            .latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
    );
    let generation = latest.generation.load(Ordering::Acquire);
    if generation == 0 || generation == record.last_seen_generation {
        return Ok(None);
    }
    let raw_texture_ptr = latest.raw_texture_ptr;
    record.last_seen_generation = generation;
    record.held = Some(latest);
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
/// `crate::desktop::DesktopVideoCapture::release_frame`'s contract. Dropping
/// the held `Arc<RingSlot>` here is the sole recycle signal — see
/// [`poll_shared_frame`]'s caveat.
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
    record.held = None;
    drop(guard);
    Ok(())
}

#[cfg(test)]
#[path = "dxgi_shared_tests.rs"]
mod tests;
