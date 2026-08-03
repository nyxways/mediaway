//! DX12 HAL escape hatch → Mediaway D3D12→D3D11 `GpuCopy` bridge.
//!
//! wgpu has no native video-encode API. This module reaches past wgpu's own
//! API via its **HAL interop escape hatches** (`wgpu::Device::as_hal`,
//! `wgpu::Device::create_texture_from_hal`) to recover the *native*
//! `ID3D12Device` / `ID3D12Resource` wgpu's DX12 backend already holds, then
//! hands that native device to the existing
//! [`mediaway_encoder::windows::D3d12SharedEncodeBridge`] (D3D12 shared heap →
//! native D3D11), so an app already rendering/compositing with wgpu can
//! encode through `mediaway-encoder-windows`'s WMF hardware encoder without a
//! GPU→CPU readback.
//!
//! **Path class: `GpuCopy`, not Zero-Copy.** wgpu has no D3D11 backend; its
//! only Windows-native backend is DX12, and Windows Media Foundation hardware
//! encoder MFTs reject `D3D11On12`-wrapped textures
//! (`MF_E_UNSUPPORTED_D3D_TYPE`, see `mediaway-encoder-windows` ADR-0006). So
//! the same one-copy-per-frame bridge ADR-0006 already ships for native D3D12
//! apps is the only real bridge available today — [`WgpuDx12Bridge::copy_frame`]
//! records one GPU→GPU `CopyResource`, then a CPU-side `device.poll(Wait)`
//! stall (the bridge's shared NT handle carries no cross-device fence yet)
//! before the caller may push the resulting handle into the encoder. See
//! [ADR-0001](../adr/0001-dx12-hal-gpucopy-bridge.md).

#![allow(unsafe_code)]

use mediaway_common::{GpuBufferHandle, GpuDeviceHandle, NativeHandle};
use mediaway_encoder::EncodeError;
// `windows_hal_interop` is the SAME `windows`-crate version (0.58.0) that
// `wgpu_hal::dx12` itself depends on internally — required to name the exact
// type `wgpu_hal::dx12::Device::raw_device`/`texture_from_raw` expect. This
// crate's other windows-typed code (talking to `mediaway-encoder-windows`)
// uses the ordinary (0.62) `windows` dependency instead — see Cargo.toml.
use windows_hal_interop::Win32::Graphics::Direct3D12::{ID3D12Device, ID3D12Resource};
use windows_hal_interop::core::Interface;

use crate::wgpu::error::WgpuInteropError;

/// Fixed pixel format the bridge's shared D3D12 texture is allocated in.
///
/// Matches [`mediaway_encoder::windows::D3d12SharedEncodeBridge`]'s own
/// `DXGI_FORMAT_B8G8R8A8_UNORM` allocation and `PixelFormat::Bgra8`'s
/// Zero-Copy DX11 input path (`mediaway-encoder-windows` ADR-0005). Callers
/// must render/hold `source` textures in this format.
pub const BRIDGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

/// wgpu DX12 HAL interop → Windows encode `GpuCopy` bridge.
///
/// Open once per encode session with a fixed `width`/`height`; call
/// [`Self::copy_frame`] once per frame. `device`/`queue` passed to
/// [`Self::copy_frame`] must be the same pair used to open this bridge via
/// [`Self::new`] — the wrapped destination texture (and the underlying
/// `ID3D12Device` the bridge opened on) are tied to that specific `wgpu`
/// device instance.
pub struct WgpuDx12Bridge {
    bridge: mediaway_encoder::windows::D3d12SharedEncodeBridge,
    /// The bridge's own shared D3D12 resource, re-wrapped as a `wgpu::Texture`
    /// once so [`Self::copy_frame`] can record an ordinary
    /// `copy_texture_to_texture` instead of hand-rolled HAL command recording.
    dest: wgpu::Texture,
    width: u32,
    height: u32,
}

impl WgpuDx12Bridge {
    /// Extract the native `ID3D12Device*` behind `device` (must be wgpu's DX12
    /// backend) and open a [`mediaway_encoder::windows::D3d12SharedEncodeBridge`]
    /// sized `width`x`height` on it.
    ///
    /// # Errors
    ///
    /// [`WgpuInteropError::HalUnavailable`] when `device` is not backed by
    /// wgpu's DX12 HAL (a Vulkan/Metal/GL/custom/`BrowserWebGpu` backend, or a
    /// build without the `dx12` wgpu feature). [`WgpuInteropError::InvalidInput`]
    /// for zero size or a null device pointer. [`WgpuInteropError::Bridge`]
    /// when the underlying D3D12/D3D11 bridge fails to open — see
    /// [`D3d12SharedEncodeBridge::open`](mediaway_encoder::windows::D3d12SharedEncodeBridge::open).
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Result<Self, WgpuInteropError> {
        if width == 0 || height == 0 {
            return Err(WgpuInteropError::InvalidInput);
        }

        // SAFETY: `as_hal`'s contract only requires the returned resource not
        // be destroyed while the guard is the last live reference; we only
        // read the raw pointer through `Interface::as_raw` below and drop the
        // guard immediately after, never touching the device destructively.
        let native_device = unsafe { device.as_hal::<wgpu::hal::api::Dx12>() }
            .ok_or(WgpuInteropError::HalUnavailable)?;
        let raw_device: &ID3D12Device = native_device.raw_device();
        let device_handle = NativeHandle::new(Interface::as_raw(raw_device) as usize)
            .ok_or(WgpuInteropError::InvalidInput)?;
        drop(native_device);

        let bridge =
            mediaway_encoder::windows::D3d12SharedEncodeBridge::open(device_handle, width, height)?;
        let dest = wrap_bridge_resource(device, &bridge, width, height)?;

        Ok(Self {
            bridge,
            dest,
            width,
            height,
        })
    }

    /// [`GpuDeviceHandle::DirectX11`] for
    /// [`mediaway_encoder::VideoEncoderConfig::gpu_device`] — the native D3D11
    /// device the bridge opened on the same adapter.
    ///
    /// # Errors
    ///
    /// [`WgpuInteropError::Bridge`] if the live COM interface's pointer is
    /// somehow null (not expected in practice).
    #[must_use = "the returned handle is required to open a Zero-Copy-input encoder"]
    pub fn gpu_device_handle(&self) -> Result<GpuDeviceHandle, WgpuInteropError> {
        Ok(GpuDeviceHandle::DirectX11(
            self.bridge.d3d11_device_handle()?,
        ))
    }

    /// Copy `source` (a caller-owned `wgpu::Texture`, [`BRIDGE_FORMAT`], same
    /// `width`/`height` as [`Self::new`]) into the bridge's shared texture and
    /// wait for the GPU copy to complete.
    ///
    /// **Costly path — `GpuCopy` plus one CPU↔GPU sync stall per frame.** Not
    /// Zero-Copy: this records `copy_texture_to_texture` on `device`'s own
    /// queue, submits it, then blocks (`device.poll(PollType::Wait)`) until
    /// that submission finishes. The stall exists because the bridge's shared
    /// NT handle carries no cross-device fence — without it, WMF's own
    /// (different) D3D11 device could race the copy. Returns a
    /// [`GpuBufferHandle::DirectX11`] ready to push into a
    /// [`mediaway_common::VideoFrame`] for that same frame.
    ///
    /// # Errors
    ///
    /// [`WgpuInteropError::InvalidInput`] when `source`'s size or format does
    /// not match the bridge. [`WgpuInteropError::Bridge`] on a `poll` failure
    /// or a bridge handle failure.
    pub fn copy_frame(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &wgpu::Texture,
    ) -> Result<GpuBufferHandle, WgpuInteropError> {
        if source.width() != self.width
            || source.height() != self.height
            || source.format() != BRIDGE_FORMAT
        {
            return Err(WgpuInteropError::InvalidInput);
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("mediaway-wgpu dx12 bridge copy"),
        });
        let extent = wgpu::Extent3d {
            width: self.width,
            height: self.height,
            depth_or_array_layers: 1,
        };
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: source,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &self.dest,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            extent,
        );
        let submission_index = queue.submit(std::iter::once(encoder.finish()));

        device
            .poll(wgpu::PollType::WaitForSubmissionIndex(submission_index))
            .map_err(|_| WgpuInteropError::Bridge(EncodeError::Backend))?;

        Ok(self.bridge.as_dx11_handle()?)
    }
}

/// Re-wrap the bridge's own shared `ID3D12Resource` as a `wgpu::Texture`
/// (once, at open time) so [`WgpuDx12Bridge::copy_frame`] can use wgpu's
/// ordinary `copy_texture_to_texture` instead of recording raw HAL commands
/// by hand every frame.
fn wrap_bridge_resource(
    device: &wgpu::Device,
    bridge: &mediaway_encoder::windows::D3d12SharedEncodeBridge,
    width: u32,
    height: u32,
) -> Result<wgpu::Texture, WgpuInteropError> {
    let handle = bridge.d3d12_resource_handle()?;
    let raw = handle.get() as *mut core::ffi::c_void;
    // SAFETY: `handle` came from a live `ID3D12Resource` the bridge owns for
    // the whole session's lifetime; `from_raw_borrowed` + `clone()` AddRefs a
    // new, independent COM reference for the hal texture wrapper to own —
    // mirrors the existing `device_from_handle` pattern in
    // `mediaway-encoder-windows`'s `wmf::dx11` module.
    let borrowed =
        unsafe { ID3D12Resource::from_raw_borrowed(&raw) }.ok_or(WgpuInteropError::InvalidInput)?;
    let resource: ID3D12Resource = borrowed.clone(); // clone: COM AddRef, hal texture takes ownership

    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    // SAFETY: `resource` is the exact single-mip, single-sample,
    // `DXGI_FORMAT_B8G8R8A8_UNORM` (matching `BRIDGE_FORMAT`), `width`x`height`
    // D3D12 texture `D3d12SharedEncodeBridge::open` allocates — the size/format
    // arguments below describe that same shape, not an assumption.
    // `texture_from_raw` is an associated function on `dx12::Device` (not
    // `Texture` — it has no `&self` receiver either; it's a free constructor
    // namespaced under `Device`), confirmed against the vendored `wgpu-hal`
    // 26.0.6 source (`src/dx12/device.rs`), not guessed from an older/newer
    // `wgpu` version's docs.
    let hal_texture = unsafe {
        wgpu::hal::dx12::Device::texture_from_raw(
            resource,
            BRIDGE_FORMAT,
            wgpu::TextureDimension::D2,
            size,
            1,
            1,
        )
    };
    let texture_desc = wgpu::TextureDescriptor {
        label: Some("mediaway-wgpu dx12 bridge dest"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: BRIDGE_FORMAT,
        usage: wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    };
    // SAFETY: `hal_texture` was just built from `texture_desc`'s exact
    // size/format/mip/sample-count parameters above, and from a resource
    // whose owning `ID3D12Device` is the same one `device.as_hal::<Dx12>()`
    // returned in `WgpuDx12Bridge::new` (the bridge opened on that extracted
    // handle) — satisfying `create_texture_from_hal`'s "created from this
    // device's internal handle" contract.
    let wgpu_texture = unsafe {
        device.create_texture_from_hal::<wgpu::hal::api::Dx12>(hal_texture, &texture_desc)
    };
    Ok(wgpu_texture)
}
