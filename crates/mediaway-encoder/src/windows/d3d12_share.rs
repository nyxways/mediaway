//! D3D12 shared heap → native D3D11 texture for WMF encode (storm-chaser pattern).
//!
//! **Path class: `GpuCopy`** — callers typically `CopyResource` into the shared D3D12
//! texture once per frame, then submit the opened [`ID3D11Texture2D`] to the encoder.
//! Do **not** use `D3D11On12` for NVENC/MF — that path yields `MF_E_UNSUPPORTED_D3D_TYPE`.
//!
//! See [ADR-0006](../adr/0006-d3d12-shared-to-d3d11.md).

#![allow(unsafe_code)]

use crate::EncodeError;
use mediaway_common::{GpuBufferHandle, NativeHandle};
use windows::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE, HMODULE};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_1;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION,
    D3D11CreateDevice, ID3D11Device, ID3D11Device1, ID3D11Resource, ID3D11Texture2D,
};
use windows::Win32::Graphics::Direct3D12::{
    D3D12_CPU_PAGE_PROPERTY_UNKNOWN, D3D12_HEAP_FLAG_SHARED, D3D12_HEAP_PROPERTIES,
    D3D12_HEAP_TYPE_DEFAULT, D3D12_MEMORY_POOL_UNKNOWN, D3D12_RESOURCE_DESC,
    D3D12_RESOURCE_DIMENSION_TEXTURE2D, D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
    D3D12_RESOURCE_STATE_COMMON, D3D12_TEXTURE_LAYOUT_UNKNOWN, ID3D12Device, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4};
use windows::core::{Interface, PCWSTR};

/// Shared D3D12 texture opened as a native D3D11 texture for MF/DXGI encode.
pub struct D3d12SharedEncodeBridge {
    d3d12_resource: ID3D12Resource,
    d3d11_device: ID3D11Device,
    d3d11_texture: ID3D11Texture2D,
    shared_handle: HANDLE,
}

impl D3d12SharedEncodeBridge {
    /// Allocate a `D3D12_HEAP_FLAG_SHARED` BGRA texture on `d3d12_device` and open it
    /// on a **native** same-adapter D3D11 device via `OpenSharedResource1`.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::InvalidInput`] for zero size, or
    /// [`EncodeError::Backend`] on D3D / DXGI failure.
    pub fn open(d3d12_device: NativeHandle, width: u32, height: u32) -> Result<Self, EncodeError> {
        if width == 0 || height == 0 {
            return Err(EncodeError::InvalidInput);
        }
        let d3d12 = device_from_handle(d3d12_device)?;

        let heap_props = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
            MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
            CreationNodeMask: 1,
            VisibleNodeMask: 1,
        };
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Alignment: 0,
            Width: u64::from(width),
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            // ALLOW_RENDER_TARGET required — bare shared heaps fail OpenSharedResource1.
            Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
        };
        let mut resource: Option<ID3D12Resource> = None;
        // SAFETY: CreateCommittedResource with SHARED heap for cross-API open.
        unsafe {
            d3d12
                .CreateCommittedResource(
                    &raw const heap_props,
                    D3D12_HEAP_FLAG_SHARED,
                    &raw const desc,
                    D3D12_RESOURCE_STATE_COMMON,
                    None,
                    &raw mut resource,
                )
                .map_err(|_| EncodeError::Backend)?;
        }
        let resource = resource.ok_or(EncodeError::Backend)?;

        Self::from_resource(&d3d12, resource)
    }

    /// Share a caller-owned `ID3D12Resource*` instead of allocating one — `resource` must
    /// already be `D3D12_HEAP_FLAG_SHARED`-allocated (`ALLOW_RENDER_TARGET` recommended for
    /// render-target use) on `d3d12_device`. No `width`/`height` parameter: unlike [`Self::open`],
    /// this does not allocate, so the resource's own dimensions apply.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::InvalidInput`] for a null device/resource pointer, or
    /// [`EncodeError::Backend`] on D3D / DXGI failure — including a `resource` that was not
    /// actually shared-heap-allocated (`CreateSharedHandle` itself fails for a non-shared
    /// resource).
    pub fn open_with_resource(
        d3d12_device: NativeHandle,
        d3d12_resource: NativeHandle,
    ) -> Result<Self, EncodeError> {
        let d3d12 = device_from_handle(d3d12_device)?;
        let raw = d3d12_resource.get() as *mut std::ffi::c_void;
        // SAFETY: caller guarantees a live, D3D12_HEAP_FLAG_SHARED-allocated ID3D12Resource*.
        let borrowed =
            unsafe { ID3D12Resource::from_raw_borrowed(&raw) }.ok_or(EncodeError::InvalidInput)?;
        // clone: COM AddRef so we own a reference for CreateSharedHandle/the struct's lifetime
        let resource: ID3D12Resource = borrowed.clone();

        Self::from_resource(&d3d12, resource)
    }

    /// Shared tail of [`Self::open`]/[`Self::open_with_resource`]: `CreateSharedHandle` on
    /// `resource`, then open it on a **native** same-adapter D3D11 device via
    /// `OpenSharedResource1`.
    fn from_resource(d3d12: &ID3D12Device, resource: ID3D12Resource) -> Result<Self, EncodeError> {
        // SAFETY: CreateSharedHandle for NT handle open on D3D11.
        let shared_handle: HANDLE = unsafe {
            d3d12
                .CreateSharedHandle(&resource, None, GENERIC_ALL.0, PCWSTR::null())
                .map_err(|_| EncodeError::Backend)?
        };

        let adapter_luid = unsafe { d3d12.GetAdapterLuid() };
        let factory: IDXGIFactory4 = unsafe {
            CreateDXGIFactory2(windows::Win32::Graphics::Dxgi::DXGI_CREATE_FACTORY_FLAGS(0))
        }
        .map_err(|_| EncodeError::Backend)?;
        let adapter: IDXGIAdapter =
            unsafe { factory.EnumAdapterByLuid(adapter_luid) }.map_err(|_| EncodeError::Backend)?;

        let mut d3d11_device: Option<ID3D11Device> = None;
        let mut feature_level = D3D_FEATURE_LEVEL::default();
        // SAFETY: same-adapter native D3D11 (not D3D11On12).
        unsafe {
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_VIDEO_SUPPORT | D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&raw mut d3d11_device),
                Some(&raw mut feature_level),
                None,
            )
        }
        .map_err(|_| EncodeError::Backend)?;
        let _ = feature_level;
        let d3d11_device = d3d11_device.ok_or(EncodeError::Backend)?;
        let device1: ID3D11Device1 = d3d11_device.cast().map_err(|_| EncodeError::Backend)?;
        // SAFETY: OpenSharedResource1 on native D3D11Device1.
        let opened: ID3D11Resource = unsafe { device1.OpenSharedResource1(shared_handle) }
            .map_err(|_| EncodeError::Backend)?;
        let d3d11_texture: ID3D11Texture2D = opened.cast().map_err(|_| EncodeError::Backend)?;

        Ok(Self {
            d3d12_resource: resource,
            d3d11_device,
            d3d11_texture,
            shared_handle,
        })
    }

    /// Opaque `ID3D12Resource*` for caller `CopyResource` into the shared heap.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::Backend`] if the live COM interface somehow yields a
    /// null pointer (not expected in practice — a valid `Interface` value is never
    /// backed by a null vtable).
    pub fn d3d12_resource_handle(&self) -> Result<NativeHandle, EncodeError> {
        to_native_handle(&self.d3d12_resource)
    }

    /// Opaque `ID3D11Device*` (use as encoder `gpu_device`).
    ///
    /// # Errors
    ///
    /// See [`Self::d3d12_resource_handle`].
    pub fn d3d11_device_handle(&self) -> Result<NativeHandle, EncodeError> {
        to_native_handle(&self.d3d11_device)
    }

    /// Opaque `ID3D11Texture2D*` for [`GpuBufferHandle::DirectX11`] encode push.
    ///
    /// # Errors
    ///
    /// See [`Self::d3d12_resource_handle`].
    pub fn d3d11_texture_handle(&self) -> Result<NativeHandle, EncodeError> {
        to_native_handle(&self.d3d11_texture)
    }

    /// Shared NT handle as [`GpuBufferHandle::DirectXShared`].
    ///
    /// # Errors
    ///
    /// See [`Self::d3d12_resource_handle`].
    pub fn shared_handle(&self) -> Result<GpuBufferHandle, EncodeError> {
        let handle =
            NativeHandle::new(self.shared_handle.0 as usize).ok_or(EncodeError::Backend)?;
        Ok(GpuBufferHandle::DirectXShared { handle })
    }

    /// Encode-ready `DirectX11` handle (subresource 0).
    ///
    /// # Errors
    ///
    /// See [`Self::d3d12_resource_handle`].
    pub fn as_dx11_handle(&self) -> Result<GpuBufferHandle, EncodeError> {
        Ok(GpuBufferHandle::DirectX11 {
            texture: self.d3d11_texture_handle()?,
            subresource: 0,
        })
    }
}

/// Borrow + clone (COM `AddRef`) a caller-supplied `ID3D12Device*` handle.
fn device_from_handle(d3d12_device: NativeHandle) -> Result<ID3D12Device, EncodeError> {
    let raw = d3d12_device.get() as *mut std::ffi::c_void;
    // SAFETY: caller guarantees a live ID3D12Device* for the session.
    let borrowed =
        unsafe { ID3D12Device::from_raw_borrowed(&raw) }.ok_or(EncodeError::InvalidInput)?;
    Ok(borrowed.clone()) // clone: COM AddRef so we own the device for this call's lifetime
}

/// Wrap a live COM interface's raw pointer as a [`NativeHandle`].
///
/// A `windows`-crate [`Interface`] value that exists at all is never backed by a
/// null vtable pointer — this returns `Result` instead of panicking so a
/// theoretical violation surfaces as [`EncodeError::Backend`], never a panic.
fn to_native_handle<T: Interface>(obj: &T) -> Result<NativeHandle, EncodeError> {
    NativeHandle::new(Interface::as_raw(obj) as usize).ok_or(EncodeError::Backend)
}

impl Drop for D3d12SharedEncodeBridge {
    fn drop(&mut self) {
        if !self.shared_handle.is_invalid() {
            let _ = unsafe { CloseHandle(self.shared_handle) };
        }
    }
}
