//! Private D3D11 device + NV12 texture plumbing for the CPU-upload NVENC path.
//!
//! [`Dx11Upload::upload_cpu_nv12`] is a genuine CPU→GPU copy (`Map`/memcpy/`Unmap` into a
//! `D3D11_USAGE_STAGING` texture, then `CopyResource` into the GPU-resident texture NVENC
//! reads) — never Zero-Copy. Named per the workspace's cost-disclosure convention, matching
//! `mediaway-encoder-windows::wmf::video::upload_cpu_nv12` and
//! `mediaway-encoder-linux::vaapi::video::upload_cpu_nv12`.
//!
//! The device and both textures are entirely private to this crate — never exposed to
//! callers as a [`mediaway_common::GpuDeviceHandle`] / [`mediaway_common::GpuBufferHandle`].
//! This stage is CPU-upload only ([`VideoInputPreference::CpuUploadOk`](
//! crate::VideoInputPreference::CpuUploadOk)); the caller never supplies a device
//! or texture. See [ADR-0001](../../adr/0001-nvenc-vendor-backend.md) 2026-07-29 addendum
//! for why this path exists (a real, hardware-verified bug in the `nvenc` crate's native
//! `NvEncCreateInputBuffer`/lock host-memory path).

use crate::EncodeError;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_WRITE, D3D11_CREATE_DEVICE_FLAG, D3D11_MAP_WRITE,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE,
    D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
    ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};

/// Open a private, headless (no swapchain) hardware D3D11 device + immediate context —
/// used only to host the NVENC session and this module's upload textures.
pub(crate) fn open_device() -> Result<(ID3D11Device, ID3D11DeviceContext), EncodeError> {
    let mut device = None;
    let mut ctx = None;
    // SAFETY: `device`/`ctx` are valid stack `Option` out-params, only read below after
    // the call returns `Ok`; no adapter/swapchain is requested (headless compute/encode use).
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE(std::ptr::null_mut()),
            D3D11_CREATE_DEVICE_FLAG(0),
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&raw mut device),
            None,
            Some(&raw mut ctx),
        )
    }
    .map_err(|_| EncodeError::Backend)?;
    let device = device.ok_or(EncodeError::Backend)?;
    let ctx = ctx.ok_or(EncodeError::Backend)?;
    Ok((device, ctx))
}

/// A CPU-writable staging texture + the GPU-resident texture NVENC registers/reads, both
/// NV12, both sized to one encode session's `width`/`height`.
pub(crate) struct Dx11Upload {
    ctx: ID3D11DeviceContext,
    staging: ID3D11Texture2D,
    gpu_texture: ID3D11Texture2D,
}

impl Dx11Upload {
    pub(crate) fn new(
        device: &ID3D11Device,
        ctx: ID3D11DeviceContext,
        width: u32,
        height: u32,
    ) -> Result<Self, EncodeError> {
        let staging = create_nv12_texture(
            device,
            width,
            height,
            D3D11_USAGE_STAGING,
            D3D11_CPU_ACCESS_WRITE.0 as u32,
            0,
        )?;
        let gpu_texture = create_nv12_texture(
            device,
            width,
            height,
            D3D11_USAGE_DEFAULT,
            0,
            D3D11_BIND_SHADER_RESOURCE.0 as u32,
        )?;
        Ok(Self {
            ctx,
            staging,
            gpu_texture,
        })
    }

    /// GPU-resident texture to register with NVENC (see `Encoder::register_resource_dx11`).
    pub(crate) const fn gpu_texture(&self) -> &ID3D11Texture2D {
        &self.gpu_texture
    }

    /// Copy CPU NV12 `data` (tightly packed: `width*height` Y bytes, then interleaved
    /// `width*height/2` UV bytes) into the staging texture (`Map`/memcpy row-by-row/`Unmap`),
    /// then `CopyResource` it into the GPU-resident texture NVENC reads. A genuine CPU→GPU
    /// copy — see module docs.
    pub(crate) fn upload_cpu_nv12(
        &self,
        data: &[u8],
        width: u32,
        height: u32,
    ) -> Result<(), EncodeError> {
        let w = width as usize;
        let h = height as usize;
        let y_plane_bytes = w * h;
        let uv_rows = h / 2;
        if data.len() < y_plane_bytes + uv_rows * w {
            return Err(EncodeError::InvalidInput);
        }

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: `staging` was created above by this struct with `D3D11_CPU_ACCESS_WRITE`;
        // `mapped` is a valid stack out-param.
        unsafe {
            self.ctx
                .Map(&self.staging, 0, D3D11_MAP_WRITE, 0, Some(&raw mut mapped))
        }
        .map_err(|_| EncodeError::Backend)?;

        let row_pitch = mapped.RowPitch as usize;
        // SAFETY: `mapped.pData` is valid for `row_pitch * (height + height/2)` bytes for
        // the duration of the map established just above; every write below stays within a
        // single row (`w` bytes) at row index `< h` (luma) or `< uv_rows` (chroma), and the
        // driver-reported `row_pitch >= width` for a successfully mapped NV12 texture.
        unsafe {
            let base = mapped.pData.cast::<u8>();
            for row in 0..h {
                let src = &data[row * w..row * w + w];
                let dst = std::slice::from_raw_parts_mut(base.add(row * row_pitch), w);
                dst.copy_from_slice(src);
            }
            let uv_base = base.add(row_pitch * h);
            for row in 0..uv_rows {
                let src_off = y_plane_bytes + row * w;
                let src = &data[src_off..src_off + w];
                let dst = std::slice::from_raw_parts_mut(uv_base.add(row * row_pitch), w);
                dst.copy_from_slice(src);
            }
        }
        // SAFETY: `staging` is currently mapped from the `Map` call above; this ends that map.
        unsafe { self.ctx.Unmap(&self.staging, 0) };
        // SAFETY: `staging` and `gpu_texture` were created above with matching dimensions
        // and format (`DXGI_FORMAT_NV12`), the contract `CopyResource` requires.
        unsafe { self.ctx.CopyResource(&self.gpu_texture, &self.staging) };
        Ok(())
    }
}

fn create_nv12_texture(
    device: &ID3D11Device,
    width: u32,
    height: u32,
    usage: D3D11_USAGE,
    cpu_access_flags: u32,
    bind_flags: u32,
) -> Result<ID3D11Texture2D, EncodeError> {
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
        Usage: usage,
        BindFlags: bind_flags,
        CPUAccessFlags: cpu_access_flags,
        MiscFlags: 0,
    };
    let mut texture = None;
    // SAFETY: `device` is a live `ID3D11Device` from `open_device`; `desc` describes a
    // valid NV12 2D texture; the out-param is a valid stack `Option`.
    unsafe { device.CreateTexture2D(&raw const desc, None, Some(&raw mut texture)) }
        .map_err(|_| EncodeError::Backend)?;
    texture.ok_or(EncodeError::Backend)
}
