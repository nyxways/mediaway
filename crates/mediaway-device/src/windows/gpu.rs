//! GPU adapter enumeration + configurable `DirectX11` device creation. See
//! [ADR-0007](../../adr/0007-gpu-device-factory.md).
//!
//! Every Zero-Copy capture/encode/decode path in this workspace expects a caller to
//! already own a live `GpuDeviceHandle` — this module is the one place that actually
//! creates one for a caller who has nothing to bring, replacing the hand-rolled raw
//! `D3D11CreateDevice` call every hardware test/example previously duplicated
//! independently (see the ADR's Context).

use crate::CaptureError;
use mediaway_common::{GpuDeviceHandle, NativeHandle};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_DEBUG, D3D11_CREATE_DEVICE_FLAG, D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
    D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIAdapter, IDXGIAdapter1, IDXGIFactory1,
};
use windows::core::Interface;

/// One physical GPU adapter DXGI reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuAdapterInfo {
    /// Position in this enumeration call's result order — pass to
    /// [`GpuAdapterSelect::Index`] to open a device against this exact adapter.
    /// Not guaranteed stable across separate calls if the adapter topology changes
    /// (a GPU is hot-added/removed) between them.
    pub index: u32,
    /// Adapter description string (`DXGI_ADAPTER_DESC1::Description`).
    pub name: String,
    /// PCI vendor ID (`DXGI_ADAPTER_DESC1::VendorId`).
    pub vendor_id: u32,
    /// PCI device ID (`DXGI_ADAPTER_DESC1::DeviceId`).
    pub device_id: u32,
    /// Bytes of dedicated video memory this adapter reports.
    pub dedicated_video_memory: u64,
    /// `false` for the WARP/software rasterizer adapter.
    pub is_hardware: bool,
}

/// List every GPU adapter DXGI can see, in enumeration order.
///
/// # Errors
///
/// Returns [`CaptureError::Backend`] on DXGI factory/adapter enumeration failure.
pub fn enumerate_gpu_adapters() -> Result<Vec<GpuAdapterInfo>, CaptureError> {
    // SAFETY: CreateDXGIFactory1 with no output pointers held past this call.
    let factory: IDXGIFactory1 =
        unsafe { CreateDXGIFactory1() }.map_err(|_| CaptureError::Backend)?;

    let mut out = Vec::new();
    for index in 0.. {
        // SAFETY: EnumAdapters1 out-param is a fresh COM interface pointer.
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(index) }) else {
            break;
        };
        // SAFETY: GetDesc1 reads a fixed-size struct with no retained pointers.
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
            continue;
        };
        out.push(GpuAdapterInfo {
            index,
            name: adapter_name(&desc.Description),
            vendor_id: desc.VendorId,
            device_id: desc.DeviceId,
            dedicated_video_memory: desc.DedicatedVideoMemory as u64,
            is_hardware: (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) == 0,
        });
    }
    Ok(out)
}

fn adapter_name(description: &[u16; 128]) -> String {
    let len = description
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(description.len());
    String::from_utf16_lossy(&description[..len])
}

/// Which adapter to open a device against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuAdapterSelect {
    /// First hardware adapter DXGI reports (skips WARP/software) — the same
    /// selection `D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, ..)` already
    /// makes implicitly.
    #[default]
    Default,
    /// An `index` from [`enumerate_gpu_adapters`].
    Index(u32),
}

/// Device-creation knobs this workspace's capture/encode/decode paths actually use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuDeviceOptions {
    /// Which adapter to open the device against.
    pub adapter: GpuAdapterSelect,
    /// `D3D11_CREATE_DEVICE_VIDEO_SUPPORT` — required by every capture/encode/decode
    /// path that takes this device.
    pub video_support: bool,
    /// `D3D11_CREATE_DEVICE_DEBUG` — real driver-side cost, opt-in only.
    pub debug_layer: bool,
}

impl Default for GpuDeviceOptions {
    /// Matches every existing hand-rolled `D3D11CreateDevice` call site in this
    /// workspace: default adapter, video support on, debug layer off.
    fn default() -> Self {
        Self {
            adapter: GpuAdapterSelect::Default,
            video_support: true,
            debug_layer: false,
        }
    }
}

/// An owned `DirectX11` device. Dropping it releases the underlying COM object —
/// [`handle`](Self::handle)'s [`GpuDeviceHandle`] bits are only valid while this
/// `GpuDevice` (or another owner of the same `ID3D11Device`) is alive, the same
/// caller-tracked-lifetime contract `GpuDeviceHandle` already documents.
pub struct GpuDevice {
    // Never read directly — kept alive only so Drop releases the COM refcount no
    // later than this `GpuDevice` itself; `handle` (below) is the value callers use.
    #[allow(dead_code, reason = "held for its Drop side effect, not read")]
    device: ID3D11Device,
    handle: NativeHandle,
}

impl GpuDevice {
    /// Create a real `DirectX11` device per `options`.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::InvalidInput`] for an out-of-range
    /// [`GpuAdapterSelect::Index`]. Returns [`CaptureError::Backend`] when
    /// `D3D11CreateDevice` itself fails (no compatible adapter, driver rejection).
    pub fn create(options: GpuDeviceOptions) -> Result<Self, CaptureError> {
        let mut flags = D3D11_CREATE_DEVICE_FLAG(0);
        if options.video_support {
            flags |= D3D11_CREATE_DEVICE_VIDEO_SUPPORT;
        }
        if options.debug_layer {
            flags |= D3D11_CREATE_DEVICE_DEBUG;
        }

        let device = match options.adapter {
            GpuAdapterSelect::Default => create_device(None, D3D_DRIVER_TYPE_HARDWARE, flags)?,
            GpuAdapterSelect::Index(index) => {
                // SAFETY: CreateDXGIFactory1 with no output pointers held past this call.
                let factory: IDXGIFactory1 =
                    unsafe { CreateDXGIFactory1() }.map_err(|_| CaptureError::Backend)?;
                // SAFETY: EnumAdapters1 out-param is a fresh COM interface pointer.
                let adapter: IDXGIAdapter1 = unsafe { factory.EnumAdapters1(index) }
                    .map_err(|_| CaptureError::InvalidInput)?;
                let adapter: IDXGIAdapter = adapter.cast().map_err(|_| CaptureError::Backend)?;
                create_device(Some(&adapter), D3D_DRIVER_TYPE_UNKNOWN, flags)?
            }
        };

        // A live `ID3D11Device`'s raw pointer is never zero; still checked (not
        // `expect`ed) rather than assumed, per this workspace's no-panics-outside-
        // tests rule.
        let raw = Interface::as_raw(&device) as usize;
        let handle = NativeHandle::new(raw).ok_or(CaptureError::Backend)?;

        Ok(Self { device, handle })
    }

    /// The `GpuDeviceHandle` bits to pass into capture/encode/decode configs.
    #[must_use]
    pub const fn handle(&self) -> GpuDeviceHandle {
        GpuDeviceHandle::DirectX11(self.handle)
    }
}

fn create_device(
    adapter: Option<&IDXGIAdapter>,
    driver_type: windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE,
    flags: D3D11_CREATE_DEVICE_FLAG,
) -> Result<ID3D11Device, CaptureError> {
    let mut device: Option<ID3D11Device> = None;
    // SAFETY: standard D3D11CreateDevice call; no adapter/context/feature-level
    // out-params retained beyond what's captured into `device` here.
    unsafe {
        D3D11CreateDevice(
            adapter,
            driver_type,
            HMODULE::default(),
            flags,
            None,
            D3D11_SDK_VERSION,
            Some(&raw mut device),
            None,
            None,
        )
    }
    .map_err(|_| CaptureError::Backend)?;
    device.ok_or(CaptureError::Backend)
}

#[cfg(test)]
#[path = "gpu_tests.rs"]
mod tests;
