//! Non-Windows stub.

use crate::{
    CaptureError, DeviceEvent, DeviceHotplug, DeviceInfo, DeviceKind, PermissionState, Support,
    Unavailable,
};
use mediaway_common::GpuDeviceHandle;

/// Windows audio hotplug stub.
pub struct WindowsDeviceHotplug {
    _priv: (),
}

impl WindowsDeviceHotplug {
    /// Unavailable off Windows.
    pub const fn open(_kinds: &[DeviceKind]) -> Result<Self, CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

impl DeviceHotplug for WindowsDeviceHotplug {
    fn poll_event(&mut self) -> Result<Option<DeviceEvent>, CaptureError> {
        Err(CaptureError::Unsupported)
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        Err(CaptureError::Unsupported)
    }
}

/// No Windows backend is compiled into this binary at all off Windows.
#[must_use]
pub const fn support(_kind: DeviceKind) -> Support {
    Support::Unavailable(Unavailable::NotImplemented)
}

/// No Windows backend is compiled into this binary at all off Windows.
pub const fn request_permission(_kind: DeviceKind) -> Result<PermissionState, CaptureError> {
    Ok(PermissionState::NotSupported)
}

/// No Windows backend is compiled into this binary at all off Windows.
pub const fn enumerate(_kind: DeviceKind) -> Result<Vec<DeviceInfo>, CaptureError> {
    Err(CaptureError::Unsupported)
}

/// GPU adapter info shape — see [ADR-0007](../../adr/0007-gpu-device-factory.md).
/// Same fields as the Windows implementation for a uniform public API; never
/// populated off Windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuAdapterInfo {
    /// Position in the enumeration call's result order.
    pub index: u32,
    /// Adapter description string.
    pub name: String,
    /// PCI vendor ID.
    pub vendor_id: u32,
    /// PCI device ID.
    pub device_id: u32,
    /// Bytes of dedicated video memory this adapter reports.
    pub dedicated_video_memory: u64,
    /// `false` for a software rasterizer adapter.
    pub is_hardware: bool,
}

/// No Windows backend is compiled into this binary at all off Windows.
pub const fn enumerate_gpu_adapters() -> Result<Vec<GpuAdapterInfo>, CaptureError> {
    Err(CaptureError::Unsupported)
}

/// See [ADR-0007](../../adr/0007-gpu-device-factory.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuAdapterSelect {
    /// First hardware adapter.
    #[default]
    Default,
    /// An `index` from [`enumerate_gpu_adapters`].
    Index(u32),
}

/// See [ADR-0007](../../adr/0007-gpu-device-factory.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuDeviceOptions {
    /// Which adapter to open the device against.
    pub adapter: GpuAdapterSelect,
    /// Whether video-decode/encode support is required.
    pub video_support: bool,
    /// Whether to enable the D3D11 debug layer.
    pub debug_layer: bool,
}

impl Default for GpuDeviceOptions {
    fn default() -> Self {
        Self {
            adapter: GpuAdapterSelect::Default,
            video_support: true,
            debug_layer: false,
        }
    }
}

/// No Windows backend is compiled into this binary at all off Windows.
pub struct GpuDevice {
    _priv: (),
}

impl GpuDevice {
    /// Unavailable off Windows.
    pub const fn create(_options: GpuDeviceOptions) -> Result<Self, CaptureError> {
        Err(CaptureError::Unsupported)
    }

    /// Unreachable in practice off Windows ([`create`](Self::create) always errs, so no
    /// live instance exists to call this on). `WebGpu { device_id: 0 }` is a safe,
    /// infallible placeholder — the only variant that needs no [`NativeHandle`](mediaway_common::NativeHandle)
    /// (whose constructor is fallible), so there is nothing to unwrap.
    pub const fn handle(&self) -> GpuDeviceHandle {
        GpuDeviceHandle::WebGpu { device_id: 0 }
    }
}
