//! GPU adapter enumeration + configurable device creation — C ABI over
//! `mediaway-device`'s `windows::{enumerate_gpu_adapters, GpuDevice}`
//! ([`mediaway-device` ADR-0007](../../../mediaway-device/adr/0007-gpu-device-factory.md)).
//!
//! The one place FFI/binding callers (which have no pre-existing GPU device to bring,
//! unlike a Rust caller embedding this crate inside an existing renderer) get a real
//! [`crate::device::types::MediawayGpuDeviceHandle`] for Screen capture
//! (`adr/0003-gpu-handle-c-abi.md` §4: no CPU fallback, a live device is enforced) or
//! GPU-input encode (`mediaway-ffi/adr/0002-gpu-frame-input-c-abi.md`).

use std::panic::{AssertUnwindSafe, catch_unwind};

use mediaway_device::CaptureError;
use mediaway_device::windows::{
    GpuAdapterSelect, GpuDevice, GpuDeviceOptions, enumerate_gpu_adapters,
};

use crate::device::status::MediawayDeviceStatus;
use crate::device::types::{
    MediawayGpuAdapterInfo, MediawayGpuAdapterSelectKind, MediawayGpuDeviceHandle,
    MediawayGpuDeviceOptions,
};

/// Opaque GPU device handle (`mediaway_gpu_device_t*` in the C header).
///
/// Owns the real device created by [`mediaway_gpu_device_create`];
/// [`mediaway_gpu_device_close`] drops it, releasing the underlying COM object.
pub struct GpuDeviceSessionHandle {
    poisoned: bool,
    inner: GpuDevice,
}

/// List every GPU adapter this machine's DXGI factory reports.
///
/// `*out_count == 0` with `Ok` is a valid "no adapters" result (though practically
/// unreachable on a real Windows machine — even a headless one reports WARP). Free the
/// returned array with [`mediaway_gpu_adapter_list_free`], not per-entry.
///
/// # Safety
///
/// `out_adapters`/`out_count` must be valid, writable, non-null out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_gpu_adapter_list(
    out_adapters: *mut *mut MediawayGpuAdapterInfo,
    out_count: *mut usize,
) -> MediawayDeviceStatus {
    if out_adapters.is_null() || out_count.is_null() {
        return MediawayDeviceStatus::InvalidArgument;
    }
    // SAFETY: both checked non-null above; caller guarantees they are writable
    // (function contract).
    unsafe {
        out_adapters.write(std::ptr::null_mut());
        out_count.write(0);
    }

    let result = catch_unwind(AssertUnwindSafe(enumerate_gpu_adapters));
    let adapters = match result {
        Ok(Ok(adapters)) => adapters,
        Ok(Err(err)) => return err.into(),
        Err(_) => return MediawayDeviceStatus::InternalPanic,
    };

    let entries: Vec<MediawayGpuAdapterInfo> = adapters
        .into_iter()
        .map(|adapter| MediawayGpuAdapterInfo {
            index: adapter.index,
            // A defensive `CString::new` failure (an embedded NUL — practically
            // unreachable for a real DXGI adapter description) produces an empty
            // name rather than dropping the whole entry.
            name: std::ffi::CString::new(adapter.name)
                .unwrap_or_default()
                .into_raw(),
            vendor_id: adapter.vendor_id,
            device_id: adapter.device_id,
            dedicated_video_memory: adapter.dedicated_video_memory,
            is_hardware: adapter.is_hardware,
        })
        .collect();

    let (ptr, len) = leak_adapter_list(entries);
    // SAFETY: checked non-null above (function contract).
    unsafe {
        out_adapters.write(ptr);
        out_count.write(len);
    }
    MediawayDeviceStatus::Ok
}

fn leak_adapter_list(entries: Vec<MediawayGpuAdapterInfo>) -> (*mut MediawayGpuAdapterInfo, usize) {
    if entries.is_empty() {
        return (std::ptr::null_mut(), 0);
    }
    let boxed = entries.into_boxed_slice();
    let len = boxed.len();
    (Box::into_raw(boxed).cast::<MediawayGpuAdapterInfo>(), len)
}

/// Reclaim and drop an adapter array previously returned by
/// [`mediaway_gpu_adapter_list`] (including each entry's `name` string). A `NULL`
/// `adapters` is a no-op.
///
/// # Safety
///
/// `adapters`/`count` must be exactly as returned by [`mediaway_gpu_adapter_list`] (or
/// `(NULL, 0)`), and must not have already been reclaimed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_gpu_adapter_list_free(
    adapters: *mut MediawayGpuAdapterInfo,
    count: usize,
) {
    if adapters.is_null() {
        return;
    }
    // SAFETY: `adapters`/`count` were produced by `mediaway_gpu_adapter_list`
    // (function contract) — reconstructs the exact `Box<[MediawayGpuAdapterInfo]>`
    // `leak_adapter_list` leaked.
    let boxed = unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(adapters, count)) };
    for entry in &boxed {
        if !entry.name.is_null() {
            // SAFETY: `name` was produced by `CString::into_raw` above and is
            // reclaimed at most once (this whole array is freed exactly once, per
            // function contract).
            drop(unsafe { std::ffi::CString::from_raw(entry.name) });
        }
    }
}

/// Create a real GPU device per `options`.
///
/// Three outcomes: (1) `Ok` — builds the handle, writes it to `*out_device`; (2) a
/// normal `Err` — no handle exists, `*out_device` is set to `NULL`, the matching
/// status is returned; (3) a caught panic — same `NULL`/
/// [`MediawayDeviceStatus::InternalPanic`] shape as (2).
///
/// # Safety
///
/// `options` must be a valid, readable [`MediawayGpuDeviceOptions`] pointer.
/// `out_device` must be a valid, writable, non-null out-parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_gpu_device_create(
    options: *const MediawayGpuDeviceOptions,
    out_device: *mut *mut GpuDeviceSessionHandle,
) -> MediawayDeviceStatus {
    if options.is_null() || out_device.is_null() {
        return MediawayDeviceStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `options` is valid for reads (function contract).
    let options = unsafe { *options };
    // SAFETY: `out_device` is checked non-null above; caller guarantees it is
    // writable (function contract).
    unsafe { out_device.write(std::ptr::null_mut()) };

    let rust_options = GpuDeviceOptions {
        adapter: match options.adapter.kind {
            MediawayGpuAdapterSelectKind::Default => GpuAdapterSelect::Default,
            MediawayGpuAdapterSelectKind::Index => GpuAdapterSelect::Index(options.adapter.index),
        },
        video_support: options.video_support,
        debug_layer: options.debug_layer,
    };

    let result: Result<GpuDevice, CaptureError> =
        catch_unwind(AssertUnwindSafe(|| GpuDevice::create(rust_options)))
            .unwrap_or(Err(CaptureError::Backend));

    match result {
        Ok(device) => {
            let handle = Box::new(GpuDeviceSessionHandle {
                poisoned: false,
                inner: device,
            });
            // SAFETY: `out_device` is checked non-null above (function contract).
            unsafe { out_device.write(Box::into_raw(handle)) };
            MediawayDeviceStatus::Ok
        }
        Err(err) => err.into(),
    }
}

/// Read the `GpuDeviceHandle` bits of a created device — pass this into Screen capture
/// or GPU-input encode configs.
///
/// # Safety
///
/// `device` must be a live pointer returned by [`mediaway_gpu_device_create`].
/// `out_handle` must be a valid, writable, non-null out-parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_gpu_device_handle(
    device: *const GpuDeviceSessionHandle,
    out_handle: *mut MediawayGpuDeviceHandle,
) -> MediawayDeviceStatus {
    if device.is_null() || out_handle.is_null() {
        return MediawayDeviceStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `device` is a valid, live handle pointer (function
    // contract).
    let device = unsafe { &*device };
    if device.poisoned {
        return MediawayDeviceStatus::HandlePoisoned;
    }
    // SAFETY: `out_handle` is checked non-null above (function contract).
    unsafe { out_handle.write(device.inner.handle().into()) };
    MediawayDeviceStatus::Ok
}

/// Close a device, releasing the underlying GPU device.
///
/// Every [`MediawayGpuDeviceHandle`] obtained from it becomes invalid the moment this
/// returns — do not use one after this call. A `NULL` `device` is a no-op.
///
/// # Safety
///
/// `device` must be a live pointer returned by [`mediaway_gpu_device_create`], not
/// already closed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_gpu_device_close(device: *mut GpuDeviceSessionHandle) {
    if device.is_null() {
        return;
    }
    // SAFETY: caller guarantees `device` is a live, not-yet-closed pointer (function
    // contract).
    drop(unsafe { Box::from_raw(device) });
}
