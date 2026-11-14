//! NV12 GPU→CPU readback through one reused staging texture ([`super::live`]).
//!
//! Costly path by design (ADR-0006 caveats): a `CopySubresourceRegion` on the immediate
//! context, a blocking `Map` (waits for the GPU), and a row copy into caller buffers.

#![allow(unsafe_code)]

use crate::DecodeError;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_STAGING, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};

/// A CPU-readable NV12 texture matching the decoder's coded size, created on first use and
/// re-created only when that size changes.
pub(super) struct Nv12Staging {
    context: ID3D11DeviceContext,
    texture: Option<(ID3D11Texture2D, u32, u32)>,
}

impl Nv12Staging {
    pub(super) fn new(device: &ID3D11Device) -> Result<Self, DecodeError> {
        // SAFETY: `device` is live; returns its immediate context (AddRef'd).
        let context = unsafe { device.GetImmediateContext() }.map_err(|_| DecodeError::Backend)?;
        Ok(Self {
            context,
            texture: None,
        })
    }

    /// Copy `size` (shown width, height) of `src[subresource]` into the planes.
    ///
    /// The caller has checked that `y` / `uv` hold `size` at their strides.
    pub(super) fn read(
        &mut self,
        device: &ID3D11Device,
        src: &ID3D11Texture2D,
        subresource: u32,
        size: (u32, u32),
        y: (&mut [u8], usize),
        uv: (&mut [u8], usize),
    ) -> Result<(), DecodeError> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: `src` is a live decoder output texture; `desc` is an out-param.
        unsafe { src.GetDesc(&raw mut desc) };
        let (coded_w, coded_h) = (desc.Width, desc.Height);
        if desc.Format != DXGI_FORMAT_NV12 || size.0 > coded_w || size.1 > coded_h {
            return Err(DecodeError::Backend);
        }
        let staging = self.staging(device, coded_w, coded_h)?;
        // SAFETY: same device, same format and size; `subresource` is the decoder's slice
        // (for NV12 one D3D11 subresource covers both planes).
        unsafe {
            self.context
                .CopySubresourceRegion(&staging, 0, 0, 0, 0, src, subresource, None);
        }
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: staging texture created with CPU read access; blocks until the copy is done.
        unsafe {
            self.context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&raw mut mapped))
        }
        .map_err(|_| DecodeError::Backend)?;
        let pitch = mapped.RowPitch as usize;
        let (w, h) = (size.0 as usize, size.1 as usize);
        let base = mapped.pData.cast::<u8>().cast_const();
        // SAFETY: while mapped, `pData` holds `pitch * coded_h` luma bytes followed by
        // `pitch * coded_h / 2` chroma bytes (NV12 layout of a mapped staging texture); every
        // read is a `w <= pitch` byte row at a row index below those heights. The destination
        // ranges were bounds-checked by the caller.
        unsafe {
            let (y_buf, y_stride) = y;
            for row in 0..h {
                let src_row = std::slice::from_raw_parts(base.add(row * pitch), w);
                y_buf[row * y_stride..row * y_stride + w].copy_from_slice(src_row);
            }
            let (uv_buf, uv_stride) = uv;
            let uv_base = base.add(pitch * coded_h as usize);
            for row in 0..h.div_ceil(2) {
                let src_row = std::slice::from_raw_parts(uv_base.add(row * pitch), w);
                uv_buf[row * uv_stride..row * uv_stride + w].copy_from_slice(src_row);
            }
        }
        // SAFETY: mapped above.
        unsafe { self.context.Unmap(&staging, 0) };
        Ok(())
    }

    fn staging(
        &mut self,
        device: &ID3D11Device,
        width: u32,
        height: u32,
    ) -> Result<ID3D11Texture2D, DecodeError> {
        if let Some((tex, w, h)) = &self.texture
            && *w == width
            && *h == height
        {
            return Ok(tex.clone()); // clone: COM AddRef for the duration of one read
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
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut tex = None;
        // SAFETY: valid NV12 staging description; out-param.
        unsafe { device.CreateTexture2D(&raw const desc, None, Some(&raw mut tex)) }
            .map_err(|_| DecodeError::Backend)?;
        let tex = tex.ok_or(DecodeError::Backend)?;
        // clone: COM AddRef, one reference cached and one returned for this read
        self.texture = Some((tex.clone(), width, height));
        Ok(tex)
    }
}
