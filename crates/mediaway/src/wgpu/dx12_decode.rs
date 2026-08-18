//! Windows decode-output → `wgpu::Texture` import bridge — the reverse direction of
//! [`crate::wgpu::dx12`]'s DX12 → WMF encode bridge.
//!
//! Reaches past `wgpu`'s own API via the same HAL interop escape hatches
//! (`wgpu::Device::as_hal`, `wgpu::Device::create_texture_from_hal`) to recover the native
//! `ID3D12Device` `wgpu`'s DX12 backend already holds, then hands it to
//! [`mediaway_decoder::windows::D3d11SharedDecodeBridge`] (D3D11 shared texture →
//! `ID3D12Device::OpenSharedHandle`) so an app already rendering/compositing with `wgpu` can
//! display or post-process a Mediaway-decoded (WMF DX11 Zero-Copy, NV12) frame without a
//! forced GPU→CPU readback.
//!
//! **Path class: `GpuCopy`, not Zero-Copy.** One `CopySubresourceRegion` (D3D11→D3D11, same
//! device) plus a bounded CPU↔GPU query/flush stall per imported frame — see
//! [`WgpuDx12DecodeBridge::import_decoded_texture`]. See
//! [ADR-0002](../adr/0002-decode-to-wgpu-texture-bridge.md).

#![allow(unsafe_code)]

use mediaway_common::{
    GpuBufferHandle, GpuDeviceHandle, NativeHandle, PixelFormat, VideoFrame, VideoFrameStorage,
};
use mediaway_decoder::windows::D3d11SharedDecodeBridge;
// `wgpu-hal` 30.x pins the same `windows`/`windows-core` 0.62 line this workspace already
// uses (unlike 26.x, which pinned 0.58 — see `dx12.rs`'s now-resolved straddle note and
// ADR-0001), so no second, separately-versioned `windows` dependency is needed here either.
use windows::Win32::Graphics::Direct3D12::{ID3D12Device, ID3D12Resource};
use windows::core::Interface;

use crate::wgpu::error::WgpuInteropError;

/// NV12, matching Windows decode output's `PixelFormat::Nv12` (WMF DX11 Zero-Copy, see
/// `mediaway-decoder-windows` ADR-0001).
///
/// Confirmed against the pinned `wgpu-types` 30.0.0 source this workspace's `Cargo.lock`
/// resolves `wgpu = "30.0"` to (`wgpu::TextureFormat::NV12` exists exactly as named, two
/// planes: `R8Unorm` luminance + `Rg8Unorm` chrominance at half width/height) — this ADR's
/// own residual risk #1 is resolved, not left as an assumption.
///
/// **Requires `wgpu::Features::TEXTURE_FORMAT_NV12`** (native-only, DX12 + Vulkan) on the
/// caller's `wgpu::Device` at `request_device` time. [`WgpuDx12DecodeBridge::new`]'s
/// `create_texture_from_hal` call itself does not check this feature — it bypasses `wgpu`'s
/// own texture-*creation* validation, the same way [`crate::wgpu::WgpuDx12Bridge`]'s BGRA8 wrap
/// does. But any later `wgpu::Texture::create_view` call a caller makes on the texture
/// [`WgpuDx12DecodeBridge::import_decoded_texture`] returns (e.g. a per-plane
/// `TextureAspect::Plane0`/`Plane1` view for sampling) **does** validate the format against
/// the device's enabled features, and fails without it.
pub const DECODE_BRIDGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::NV12;

/// wgpu DX12 HAL interop → Windows decode-output `GpuCopy` import bridge.
///
/// Open once per decode session with a fixed `width`/`height`; call
/// [`Self::import_decoded_texture`] once per decoded frame. The returned `wgpu::Texture` is
/// this bridge's own **single, persistently-owned, reused** destination resource — see
/// [`Self::import_decoded_texture`]'s doc for the footgun this implies.
pub struct WgpuDx12DecodeBridge {
    bridge: D3d11SharedDecodeBridge,
    /// The bridge's own shared D3D12 resource, wrapped as a `wgpu::Texture` exactly **once**,
    /// at [`Self::new`] time — mirrors [`crate::wgpu::dx12::WgpuDx12Bridge`]'s `dest` field. Every
    /// [`Self::import_decoded_texture`] call copies into the same underlying D3D12 allocation
    /// and hands back a fresh `wgpu::Texture` handle (cheap `Clone`, Arc-backed) pointing at
    /// it — never a distinct resource.
    texture: wgpu::Texture,
    width: u32,
    height: u32,
}

impl WgpuDx12DecodeBridge {
    /// Extract the native `ID3D12Device*` behind `device` (must be wgpu's DX12 backend), open
    /// a [`D3d11SharedDecodeBridge`] sized `width`x`height` bridging from `d3d11_device` (the
    /// SAME device the caller opened its decode session on — not a freshly created one), and
    /// wrap its shared D3D12 resource once as an owned `wgpu::Texture`
    /// ([`DECODE_BRIDGE_FORMAT`]).
    ///
    /// # Errors
    ///
    /// [`WgpuInteropError::InvalidInput`] for zero size, a `d3d11_device` that is not
    /// [`GpuDeviceHandle::DirectX11`] (the only Windows decode-device variant this bridge
    /// reads from), or a null extracted device pointer.
    /// [`WgpuInteropError::HalUnavailable`] when `device` is not backed by wgpu's DX12 HAL.
    /// [`WgpuInteropError::DecodeBridge`] when the underlying `D3d11SharedDecodeBridge` fails
    /// to open — including an adapter-LUID mismatch between `d3d11_device` and the extracted
    /// `ID3D12Device` (`D3d11SharedDecodeBridge::open`'s own two-sided LUID check folds that
    /// case into `DecodeError::InvalidInput`, surfaced here via `DecodeBridge` rather than
    /// [`WgpuInteropError::AdapterMismatch`] — see that variant's own doc).
    pub fn new(
        device: &wgpu::Device,
        d3d11_device: GpuDeviceHandle,
        width: u32,
        height: u32,
    ) -> Result<Self, WgpuInteropError> {
        if width == 0 || height == 0 {
            return Err(WgpuInteropError::InvalidInput);
        }
        let GpuDeviceHandle::DirectX11(d3d11_handle) = d3d11_device else {
            return Err(WgpuInteropError::InvalidInput);
        };

        // SAFETY: `as_hal`'s contract only requires the returned resource not be destroyed
        // while the guard is the last live reference; we only read the raw pointer through
        // `Interface::as_raw` below and drop the guard immediately after, never touching the
        // device destructively — identical use to `WgpuDx12Bridge::new` (dx12.rs).
        let native_device = unsafe { device.as_hal::<wgpu::hal::api::Dx12>() }
            .ok_or(WgpuInteropError::HalUnavailable)?;
        let raw_device: &ID3D12Device = native_device.raw_device();
        let d3d12_handle = NativeHandle::new(Interface::as_raw(raw_device) as usize)
            .ok_or(WgpuInteropError::InvalidInput)?;
        drop(native_device);

        let bridge = D3d11SharedDecodeBridge::open(d3d11_handle, d3d12_handle, width, height)?;
        let texture = wrap_bridge_resource(device, &bridge, width, height)?;

        Ok(Self {
            bridge,
            texture,
            width,
            height,
        })
    }

    /// Copy `frame`'s decode-output GPU texture into this bridge's shared texture and return
    /// it as a `wgpu::Texture`.
    ///
    /// Validates `frame.width`/`frame.height` match [`Self::new`]'s size,
    /// `frame.format == PixelFormat::Nv12`, and `frame.storage` is
    /// `VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 { .. })`. `frame.pts`/`frame.duration`
    /// are **not** carried into the returned `wgpu::Texture` — wgpu has no timestamp concept;
    /// callers who need timing must track it out of band, keyed off the same `frame`.
    ///
    /// **Costly path — `GpuCopy` plus one CPU↔GPU query/flush stall per frame, not
    /// Zero-Copy.** [`D3d11SharedDecodeBridge::copy_from_decoded`] records a
    /// `CopySubresourceRegion` on the decode texture's own D3D11 device/context, then blocks
    /// (bounded, ~500ms deadline) on a `D3D11_QUERY_EVENT` poll before returning — so the
    /// D3D11 copy is confirmed retired before control returns here, and before any
    /// wgpu-recorded command referencing the shared resource could be submitted.
    ///
    /// **Footgun — the returned `wgpu::Texture` is the SAME underlying GPU allocation on
    /// every call, reused and overwritten each `import_decoded_texture`.** This is a sharper
    /// trap than [`crate::wgpu::WgpuDx12Bridge`]'s analogous single-buffered `dest` (encode's
    /// immediate push-and-forget vs. decode output that is often sampled across multiple
    /// render frames): holding a `wgpu::Texture` returned from one call while calling
    /// `import_decoded_texture` again observes the **second** frame's content, not a stable
    /// snapshot. Callers needing two live decoded frames simultaneously (cross-fade,
    /// double-buffering) must open two `WgpuDx12DecodeBridge` instances, or copy out of the
    /// returned texture into their own persistent one — this bridge is a single-buffered
    /// staging resource, not a frame queue.
    ///
    /// # Errors
    ///
    /// [`WgpuInteropError::InvalidInput`] when `frame`'s size, format, or storage variant does
    /// not match the bridge. [`WgpuInteropError::DecodeBridge`] when the underlying D3D11 copy
    /// or query poll fails (including exceeding its bounded deadline).
    pub fn import_decoded_texture(
        &self,
        frame: &VideoFrame,
    ) -> Result<wgpu::Texture, WgpuInteropError> {
        if frame.width != self.width
            || frame.height != self.height
            || frame.format != PixelFormat::Nv12
        {
            return Err(WgpuInteropError::InvalidInput);
        }
        let VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 {
            texture,
            subresource,
        }) = &frame.storage
        else {
            return Err(WgpuInteropError::InvalidInput);
        };

        self.bridge.copy_from_decoded(*texture, *subresource)?;

        // clone: `wgpu::Texture` is an Arc-backed dispatch handle (cheap refcount bump), not a
        // GPU-side copy — this hands the caller a second owning reference to the SAME
        // persistently-owned destination texture (see the struct field doc and this method's
        // "Footgun" note above), which this bridge keeps its own reference to for the next call.
        Ok(self.texture.clone())
    }
}

/// Re-wrap the bridge's own shared `ID3D12Resource` as a `wgpu::Texture` (once, at open time)
/// — mirrors `dx12.rs::wrap_bridge_resource` exactly, substituting the decode-direction
/// bridge type and [`DECODE_BRIDGE_FORMAT`] (NV12) for the encode direction's BGRA8.
fn wrap_bridge_resource(
    device: &wgpu::Device,
    bridge: &D3d11SharedDecodeBridge,
    width: u32,
    height: u32,
) -> Result<wgpu::Texture, WgpuInteropError> {
    let handle = bridge.d3d12_resource_handle()?;
    let raw = handle.get() as *mut core::ffi::c_void;
    // SAFETY: `handle` came from a live `ID3D12Resource` the bridge owns for the whole
    // session's lifetime; `from_raw_borrowed` + `clone()` AddRefs a new, independent COM
    // reference for the hal texture wrapper to own — identical pattern to
    // `dx12.rs::wrap_bridge_resource`.
    let borrowed =
        unsafe { ID3D12Resource::from_raw_borrowed(&raw) }.ok_or(WgpuInteropError::InvalidInput)?;
    let resource: ID3D12Resource = borrowed.clone(); // clone: COM AddRef, hal texture takes ownership

    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    // SAFETY: `resource` is the exact single-mip, single-sample, DXGI_FORMAT_NV12
    // (matching `DECODE_BRIDGE_FORMAT`), `width`x`height` texture `D3d11SharedDecodeBridge::open`
    // allocates on D3D11 and opens on D3D12 — the size/format arguments below describe that
    // same shape, not an assumption. `texture_from_raw` is an associated function on
    // `dx12::Device` (confirmed against the vendored `wgpu-hal` 30.0.0 source,
    // `src/dx12/device.rs`), the same call `WgpuDx12Bridge::wrap_bridge_resource` already uses
    // and this workspace hardware-verified for the encode direction.
    let hal_texture = unsafe {
        wgpu::hal::dx12::Device::texture_from_raw(
            resource,
            DECODE_BRIDGE_FORMAT,
            wgpu::TextureDimension::D2,
            size,
            1,
            1,
        )
    };
    let texture_desc = wgpu::TextureDescriptor {
        label: Some("mediaway-wgpu dx12 decode bridge dest"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DECODE_BRIDGE_FORMAT,
        // TEXTURE_BINDING so callers can create per-plane sampling views (the intended
        // consumer use — `D3d11SharedDecodeBridge::open` binds its D3D11 texture as
        // `D3D11_BIND_SHADER_RESOURCE` only, see that crate's ADR-0003 § Residual risk 5,
        // which is why this bridge does not request COPY_DST/RENDER_TARGET usage). COPY_SRC
        // so a caller can copy out into their own persistent texture per the
        // single-buffered-footgun workaround documented on `import_decoded_texture`.
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    };
    // SAFETY: `hal_texture` was just built from `texture_desc`'s exact size/format/mip/
    // sample-count parameters above, and from a resource whose owning `ID3D12Device` is the
    // same one `device.as_hal::<Dx12>()` returned in `WgpuDx12DecodeBridge::new` (the
    // companion `D3d11SharedDecodeBridge` opened its shared handle on that extracted handle)
    // — satisfying `create_texture_from_hal`'s "created from this device's internal handle"
    // contract. `initial_state: UNINITIALIZED` per wgpu 30's own doc guidance — this texture's
    // contents are set by the first `import_decoded_texture` copy, never read before that.
    let wgpu_texture = unsafe {
        device.create_texture_from_hal::<wgpu::hal::api::Dx12>(
            hal_texture,
            &texture_desc,
            wgpu::TextureUses::UNINITIALIZED,
        )
    };
    Ok(wgpu_texture)
}
