//! D3D11 shared texture → D3D12 open for `mediaway-wgpu` decode interop.
//!
//! **Path class: `GpuCopy`** — [`D3d11SharedDecodeBridge::copy_from_decoded`] does one
//! `CopySubresourceRegion` (D3D11→D3D11, same device) into this bridge's own shared NV12
//! texture, then a bounded CPU↔GPU query/flush poll before returning. Not Zero-Copy.
//!
//! See [ADR-0003](../adr/0003-d3d11-shared-decode-bridge.md).

#![allow(unsafe_code)]

use crate::DecodeError;
use mediaway_common::NativeHandle;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_SHADER_RESOURCE, D3D11_QUERY_DESC, D3D11_QUERY_EVENT, D3D11_RESOURCE_MISC_SHARED,
    D3D11_RESOURCE_MISC_SHARED_NTHANDLE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, ID3D11Device,
    ID3D11DeviceContext, ID3D11Query, ID3D11Texture2D,
};
use windows::Win32::Graphics::Direct3D12::{ID3D12Device, ID3D12Resource};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    DXGI_SHARED_RESOURCE_READ, DXGI_SHARED_RESOURCE_WRITE, IDXGIDevice, IDXGIResource1,
};
use windows::core::{Interface, PCWSTR};

/// Shared D3D11 NV12 texture opened as a native D3D12 resource.
///
/// Hands WMF DX11 Zero-Copy decode output (see `wmf::dx11`,
/// [ADR-0001](../adr/0001-wmf-h264-dx11-out.md)) to a caller-owned `ID3D12Device` (e.g.
/// `mediaway-wgpu`'s `WgpuDx12DecodeBridge`).
///
/// Both devices are caller-owned (unlike `mediaway-encoder-windows`'s
/// `D3d12SharedEncodeBridge`, which creates its own native D3D11 device) — `open` performs
/// a two-sided adapter LUID check instead of relying on construction order to guarantee a
/// same-adapter pair. `d3d12_device` itself is not retained past `open`.
pub struct D3d11SharedDecodeBridge {
    d3d12_resource: ID3D12Resource,
    d3d11_device: ID3D11Device,
    d3d11_context: ID3D11DeviceContext,
    d3d11_texture: ID3D11Texture2D,
    shared_handle: HANDLE,
}

impl D3d11SharedDecodeBridge {
    /// Allocate a shared NV12 `ID3D11Texture2D` on `d3d11_device` and open it as an
    /// `ID3D12Resource` on `d3d12_device` — both caller-owned, same-adapter.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::InvalidInput`] for zero size, a device handle that fails to
    /// borrow, or an adapter LUID mismatch between the two devices. Returns
    /// [`DecodeError::Backend`] on D3D11 / D3D12 / DXGI failure.
    pub fn open(
        d3d11_device: NativeHandle,
        d3d12_device: NativeHandle,
        width: u32,
        height: u32,
    ) -> Result<Self, DecodeError> {
        if width == 0 || height == 0 {
            return Err(DecodeError::InvalidInput);
        }

        let raw11 = d3d11_device.get() as *mut std::ffi::c_void;
        // SAFETY: caller guarantees a live ID3D11Device* for the decode session.
        let borrowed11 =
            unsafe { ID3D11Device::from_raw_borrowed(&raw11) }.ok_or(DecodeError::InvalidInput)?;
        // clone: COM AddRef for session-owned device handle
        let d3d11_device: ID3D11Device = borrowed11.clone();

        let raw12 = d3d12_device.get() as *mut std::ffi::c_void;
        // SAFETY: caller guarantees a live ID3D12Device* for this call.
        let borrowed12 =
            unsafe { ID3D12Device::from_raw_borrowed(&raw12) }.ok_or(DecodeError::InvalidInput)?;
        // clone: COM AddRef for session-owned device handle
        let d3d12_device: ID3D12Device = borrowed12.clone();

        // Two-sided same-adapter LUID check (see ADR-0003 § Context — both devices are
        // caller-owned here, unlike the encode-direction sibling bridge).
        // SAFETY: GetAdapterLuid is a trivial property query on a live device.
        let d3d12_luid = unsafe { d3d12_device.GetAdapterLuid() };
        let dxgi_device: IDXGIDevice = d3d11_device.cast().map_err(|_| DecodeError::Backend)?;
        // SAFETY: GetAdapter is proven, compiling precedent (mediaway-device-windows/src/dxgi.rs).
        let adapter = unsafe { dxgi_device.GetAdapter() }.map_err(|_| DecodeError::Backend)?;
        // SAFETY: GetDesc reads the adapter's fixed description block; no precondition beyond
        // a live adapter.
        let adapter_desc = unsafe { adapter.GetDesc() }.map_err(|_| DecodeError::Backend)?;
        let d3d11_luid = adapter_desc.AdapterLuid;
        if d3d11_luid.LowPart != d3d12_luid.LowPart || d3d11_luid.HighPart != d3d12_luid.HighPart {
            return Err(DecodeError::InvalidInput);
        }

        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_NV12,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            MiscFlags: D3D11_RESOURCE_MISC_SHARED.0 as u32
                | D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0 as u32,
            CPUAccessFlags: 0,
        };
        let mut texture: Option<ID3D11Texture2D> = None;
        // SAFETY: CreateTexture2D with `None` initial data — first real write happens in
        // copy_from_decoded, not here.
        unsafe {
            d3d11_device
                .CreateTexture2D(&raw const desc, None, Some(&raw mut texture))
                .map_err(|_| DecodeError::Backend)?;
        }
        let d3d11_texture = texture.ok_or(DecodeError::Backend)?;

        let resource1: IDXGIResource1 = d3d11_texture.cast().map_err(|_| DecodeError::Backend)?;
        // SAFETY: CreateSharedHandle on our own freshly created shared texture.
        let shared_handle: HANDLE = unsafe {
            resource1
                .CreateSharedHandle(
                    None,
                    DXGI_SHARED_RESOURCE_READ.0 | DXGI_SHARED_RESOURCE_WRITE.0,
                    PCWSTR::null(),
                )
                .map_err(|_| DecodeError::Backend)?
        };

        let mut d3d12_resource: Option<ID3D12Resource> = None;
        // SAFETY: opening the shared handle on D3D12 only after the LUID check above
        // confirmed same-adapter — cross-adapter open is undefined, not just slow.
        unsafe {
            d3d12_device
                .OpenSharedHandle(shared_handle, &raw mut d3d12_resource)
                .map_err(|_| DecodeError::Backend)?;
        }
        let d3d12_resource = d3d12_resource.ok_or(DecodeError::Backend)?;

        // SAFETY: GetImmediateContext is a simple accessor on a live device.
        let d3d11_context =
            unsafe { d3d11_device.GetImmediateContext() }.map_err(|_| DecodeError::Backend)?;

        Ok(Self {
            d3d12_resource,
            d3d11_device,
            d3d11_context,
            d3d11_texture,
            shared_handle,
        })
    }

    /// Copy `subresource` of a decoded `ID3D11Texture2D` (must live on this bridge's own
    /// `d3d11_device`) into the shared texture, and block (bounded) until the GPU copy
    /// retires.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::InvalidInput`] if `texture` fails to borrow, or if it lives on
    /// a different D3D11 device than this bridge. Returns [`DecodeError::Backend`] on D3D11
    /// failure, or if the query poll exceeds its deadline.
    pub fn copy_from_decoded(
        &self,
        texture: NativeHandle,
        subresource: u32,
    ) -> Result<(), DecodeError> {
        let raw = texture.get() as *mut std::ffi::c_void;
        // SAFETY: caller guarantees a live ID3D11Texture2D* for the duration of this call.
        let borrowed =
            unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }.ok_or(DecodeError::InvalidInput)?;

        // New runtime check beyond the sibling ADR's literal contract: `texture` carries no
        // type-level device tag, so this is the only cheap way to catch a cross-device
        // texture before it hits undefined `CopySubresourceRegion` behavior.
        // SAFETY: GetDevice is a simple accessor; the returned device is AddRef'd by the API
        // and dropped at the end of this scope (only its raw pointer is compared).
        let owning_device = unsafe { borrowed.GetDevice() }.map_err(|_| DecodeError::Backend)?;
        if Interface::as_raw(&owning_device) != Interface::as_raw(&self.d3d11_device) {
            return Err(DecodeError::InvalidInput);
        }

        // SAFETY: full-region copy (None box) into our own shared texture, destination mip 0
        // / array slice 0 (this bridge's texture is always MipLevels: 1, ArraySize: 1).
        // Source subresource / dimensions / NV12-ness are trusted from the decode contract,
        // same trust boundary D3d12SharedEncodeBridge's own CopyResource callers accept.
        unsafe {
            self.d3d11_context.CopySubresourceRegion(
                &self.d3d11_texture,
                0,
                0,
                0,
                0,
                borrowed,
                subresource,
                None,
            );
        }

        let query_desc = D3D11_QUERY_DESC {
            Query: D3D11_QUERY_EVENT,
            MiscFlags: 0,
        };
        let mut query: Option<ID3D11Query> = None;
        // SAFETY: a fresh query object per call — deliberate, see ADR-0003 § Alternatives.
        unsafe {
            self.d3d11_device
                .CreateQuery(&raw const query_desc, Some(&raw mut query))
                .map_err(|_| DecodeError::Backend)?;
        }
        let query = query.ok_or(DecodeError::Backend)?;

        // SAFETY: End marks the query's retirement point immediately after the copy above,
        // so it covers exactly this CopySubresourceRegion.
        unsafe { self.d3d11_context.End(&query) };
        // SAFETY: queries alone do not force submission; Flush() is required before the GPU
        // will ever retire the query.
        unsafe { self.d3d11_context.Flush() };

        // Poll loop identical in shape to `wmf::dx11::wait_need_input`: 500ms deadline, 1ms
        // sleep between polls. `GetData`'s `Result<()>` alone cannot distinguish S_OK (data
        // ready) from S_FALSE (not ready yet — still a "success" HRESULT), so the actual BOOL
        // out-param is what decides readiness here.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        loop {
            let mut done: i32 = 0;
            // SAFETY: `done` is a valid, correctly sized (4-byte) BOOL out-param for the
            // duration of this call.
            let poll = unsafe {
                self.d3d11_context.GetData(
                    &query,
                    Some((&raw mut done).cast::<std::ffi::c_void>()),
                    u32::try_from(std::mem::size_of::<i32>()).unwrap_or(4),
                    0,
                )
            };
            if poll.is_ok() && done != 0 {
                return Ok(());
            }
            if std::time::Instant::now() > deadline {
                return Err(DecodeError::Backend);
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// Opaque `ID3D12Resource*` for the caller to wrap (e.g. `wgpu::Texture` via
    /// `create_texture_from_hal`).
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::Backend`] if the live COM interface somehow yields a null
    /// pointer (not expected in practice — a valid `Interface` value is never backed by a
    /// null vtable).
    pub fn d3d12_resource_handle(&self) -> Result<NativeHandle, DecodeError> {
        to_native_handle(&self.d3d12_resource)
    }
}

/// Wrap a live COM interface's raw pointer as a [`NativeHandle`].
///
/// Not shared with `mediaway-encoder-windows`'s own copy of this helper (two-line,
/// two-call-site — no ADR-worthy abstraction, per ADR-0003 § `d3d12_resource_handle`).
fn to_native_handle<T: Interface>(obj: &T) -> Result<NativeHandle, DecodeError> {
    NativeHandle::new(Interface::as_raw(obj) as usize).ok_or(DecodeError::Backend)
}

impl Drop for D3d11SharedDecodeBridge {
    fn drop(&mut self) {
        if !self.shared_handle.is_invalid() {
            // SAFETY: shared_handle is a raw NT handle from CreateSharedHandle, not a COM
            // object — needs a manual CloseHandle. d3d12_resource/d3d11_device/
            // d3d11_context/d3d11_texture are windows-crate COM wrappers whose own Drop
            // already calls Release.
            let _ = unsafe { CloseHandle(self.shared_handle) };
        }
    }
}

#[cfg(test)]
#[path = "d3d11_shared_decode_bridge_tests.rs"]
mod tests;
