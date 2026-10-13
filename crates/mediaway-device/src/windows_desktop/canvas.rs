//! A fixed-size BGRA canvas on the GPU that pictures of any size are placed on, centered, the
//! rest black: so a capture whose size changes (a window being resized) can feed an encoder whose
//! size was fixed when it opened.
//!
//! Each placement goes into the next slot of a small ring of canvas-sized textures, for the same
//! reason as the WGC region copy's ring (`wgc.rs`, `CROP_RING_DEPTH`): an encoder may still be
//! reading a texture it was handed after the caller moved on. A slot is cleared to black only
//! when what it held is not covered by the new picture, so a picture that keeps its size costs
//! one `CopySubresourceRegion` and nothing else.
//!
//! A picture larger than the canvas is cut to it around its center. Offsets are even, so a 4:2:0
//! encoder's chroma lines up with the picture's edges.
//!
//! Or a fixed part of each picture is placed instead (`place_texture_part`): the canvas is the
//! part's size, and the part keeps its offset from the picture's top-left whatever the picture's
//! size, so recording a part of a window survives the window being resized. What of the part the
//! picture no longer reaches (it shrank past it) is black.

#![allow(unsafe_code)]

use mediaway_common::{GpuBufferHandle, NativeHandle, PixelFormat, VideoFrame, VideoFrameStorage};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_BOX, D3D11_CPU_ACCESS_READ,
    D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    D3D11_USAGE_STAGING, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread,
    ID3D11RenderTargetView, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::core::Interface;

use crate::CaptureError;

/// How many canvas slots are cycled through; see the module docs.
const RING_DEPTH: usize = 4;

/// Where a picture went on a canvas, and which part of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CanvasPlacement {
    /// Left edge on the canvas.
    pub x: u32,
    /// Top edge on the canvas.
    pub y: u32,
    /// Width on the canvas: the picture's, or the canvas's if the picture is wider.
    pub width: u32,
    /// Height on the canvas: the picture's, or the canvas's if the picture is taller.
    pub height: u32,
    /// Left edge of the part of the picture shown (non-zero only for a picture wider than the
    /// canvas).
    pub source_x: u32,
    /// Top edge of the part of the picture shown.
    pub source_y: u32,
}

impl CanvasPlacement {
    /// Where a `content` picture goes on a `canvas`: centered, even offsets; cut around its center
    /// to the canvas on an axis where it is larger.
    #[must_use]
    pub const fn centered(canvas: (u32, u32), content: (u32, u32)) -> Self {
        let (x, source_x, width) = center(canvas.0, content.0);
        let (y, source_y, height) = center(canvas.1, content.1);
        Self {
            x,
            y,
            width,
            height,
            source_x,
            source_y,
        }
    }

    /// Where the part of a `content` picture that starts at `origin` (from the picture's top-left)
    /// goes on a `canvas`: at the canvas's top-left, as much of it as both the canvas and the
    /// picture hold. A part that runs past the picture's right or bottom edge (the picture shrank)
    /// is shorter there, and black on the canvas; one that starts outside the picture is empty
    /// ([`Self::is_empty`]): an all-black canvas.
    #[must_use]
    pub const fn part(canvas: (u32, u32), content: (u32, u32), origin: (u32, u32)) -> Self {
        let width = min(canvas.0, content.0.saturating_sub(origin.0));
        let height = min(canvas.1, content.1.saturating_sub(origin.1));
        let empty = width == 0 || height == 0;
        Self {
            x: 0,
            y: 0,
            width: if empty { 0 } else { width },
            height: if empty { 0 } else { height },
            source_x: origin.0,
            source_y: origin.1,
        }
    }

    /// Whether nothing of the picture is on the canvas.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Whether this placement covers all of `other` on the canvas.
    #[must_use]
    pub const fn covers(&self, other: &Self) -> bool {
        self.x <= other.x
            && self.y <= other.y
            && self.x + self.width >= other.x + other.width
            && self.y + self.height >= other.y + other.height
    }
}

const fn min(a: u32, b: u32) -> u32 {
    if a < b { a } else { b }
}

/// One axis of [`CanvasPlacement::centered`]: where it goes, where it is cut from, how long.
const fn center(canvas: u32, content: u32) -> (u32, u32, u32) {
    if content <= canvas {
        (((canvas - content) / 2) & !1, 0, content)
    } else {
        (0, ((content - canvas) / 2) & !1, canvas)
    }
}

/// A placed picture: the canvas-sized frame for an encoder, and where the picture is on it.
#[derive(Debug, Clone)]
pub struct CanvasFrame {
    /// The canvas: a `Bgra8` Direct3D 11 texture, valid until the canvas has been used
    /// `RING_DEPTH` (4) more times or is dropped.
    pub frame: VideoFrame,
    /// Where the picture went.
    pub placed: CanvasPlacement,
}

/// What a slot holds that is not black.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Holds {
    Black,
    Picture(CanvasPlacement),
    Anything,
}

struct Slot {
    texture: ID3D11Texture2D,
    view: ID3D11RenderTargetView,
    holds: Holds,
}

/// A canvas and its ring of slots on one D3D11 device.
pub struct Canvas {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    size: (u32, u32),
    slots: Vec<Slot>,
    next: usize,
    last: Option<usize>,
    staging: Option<ID3D11Texture2D>,
}

impl Canvas {
    /// A black `width` x `height` canvas on `device` (an `ID3D11Device*`, which must outlive it).
    ///
    /// # Errors
    /// [`CaptureError::InvalidInput`] for a zero size or a null device, [`CaptureError::Backend`]
    /// when the textures cannot be made.
    pub fn new(device: NativeHandle, width: u32, height: u32) -> Result<Self, CaptureError> {
        if width == 0 || height == 0 {
            return Err(CaptureError::InvalidInput);
        }
        let raw = device.get() as *mut std::ffi::c_void;
        // SAFETY: the caller hands in a live ID3D11Device*; it is only borrowed to AddRef it.
        let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }
            .ok_or(CaptureError::InvalidInput)?
            // clone: COM AddRef, the canvas keeps the device past the caller's borrow
            .clone();
        // The copies run on the caller's thread while an encoder may use the device from its
        // own; as the WGC region copy does. Idempotent.
        if let Ok(mt) = device.cast::<ID3D11Multithread>() {
            // SAFETY: plain flag setter on a live device interface.
            let _ = unsafe { mt.SetMultithreadProtected(true) };
        }
        // SAFETY: plain getter on a live device.
        let context = unsafe { device.GetImmediateContext() }.map_err(|_| CaptureError::Backend)?;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut slots = Vec::with_capacity(RING_DEPTH);
        for _ in 0..RING_DEPTH {
            let mut texture = None;
            // SAFETY: `desc` is a valid, fully initialised description; no initial data.
            unsafe { device.CreateTexture2D(&raw const desc, None, Some(&raw mut texture)) }
                .map_err(|_| CaptureError::Backend)?;
            let texture = texture.ok_or(CaptureError::Backend)?;
            let mut view = None;
            // SAFETY: a live texture made with the render-target bind flag; default view.
            unsafe { device.CreateRenderTargetView(&texture, None, Some(&raw mut view)) }
                .map_err(|_| CaptureError::Backend)?;
            let view = view.ok_or(CaptureError::Backend)?;
            // SAFETY: a live view on this device's context.
            unsafe { context.ClearRenderTargetView(&view, &BLACK) };
            slots.push(Slot {
                texture,
                view,
                holds: Holds::Black,
            });
        }
        Ok(Self {
            device,
            context,
            size: (width, height),
            slots,
            next: 0,
            last: None,
            staging: None,
        })
    }

    /// The canvas's size.
    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Places the top-left `content` part of the BGRA texture `source` (an `ID3D11Texture2D*` on
    /// this canvas's device, at least that large) centered on the canvas: one GPU copy.
    ///
    /// # Errors
    /// [`CaptureError::InvalidInput`] for a null texture or an empty picture.
    pub fn place_texture(
        &mut self,
        source: NativeHandle,
        content: (u32, u32),
    ) -> Result<CanvasFrame, CaptureError> {
        if content.0 == 0 || content.1 == 0 {
            return Err(CaptureError::InvalidInput);
        }
        let placed = CanvasPlacement::centered(self.size, content);
        self.copy_texture(source, placed)
    }

    /// Places the part of the BGRA texture `source` (an `ID3D11Texture2D*` on this canvas's
    /// device, holding a `content` picture at its top-left) that starts at `origin` at the
    /// canvas's top-left ([`CanvasPlacement::part`]): one GPU copy, or none and an all-black canvas
    /// when the part is outside the picture.
    ///
    /// # Errors
    /// [`CaptureError::InvalidInput`] for a null texture or an empty picture.
    pub fn place_texture_part(
        &mut self,
        source: NativeHandle,
        content: (u32, u32),
        origin: (u32, u32),
    ) -> Result<CanvasFrame, CaptureError> {
        if content.0 == 0 || content.1 == 0 {
            return Err(CaptureError::InvalidInput);
        }
        let placed = CanvasPlacement::part(self.size, content, origin);
        self.copy_texture(source, placed)
    }

    /// Copies `placed`'s part of `source` onto the next slot.
    fn copy_texture(
        &mut self,
        source: NativeHandle,
        placed: CanvasPlacement,
    ) -> Result<CanvasFrame, CaptureError> {
        let raw = source.get() as *mut std::ffi::c_void;
        // SAFETY: the caller hands in a live ID3D11Texture2D*, borrowed for this call only.
        let source = unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }
            .ok_or(CaptureError::InvalidInput)?;
        if placed.is_empty() {
            let slot = self.take_slot(Holds::Black);
            return self.frame(slot, placed);
        }
        let slot = self.take_slot(Holds::Picture(placed));
        let area = D3D11_BOX {
            left: placed.source_x,
            top: placed.source_y,
            front: 0,
            right: placed.source_x + placed.width,
            bottom: placed.source_y + placed.height,
            back: 1,
        };
        // SAFETY: both textures are live on the same device and BGRA; `area` lies inside the
        // source (the caller vouches for the picture's size, and `centered` and `part` keep the
        // area inside it) and lands inside the slot (both keep it on the canvas).
        unsafe {
            self.context.CopySubresourceRegion(
                &self.slots[slot].texture,
                0,
                placed.x,
                placed.y,
                0,
                source,
                0,
                Some(&raw const area),
            );
        }
        self.frame(slot, placed)
    }

    /// Places a `width` x `height` BGRA picture in memory (rows of `width * 4` bytes) centered on
    /// the canvas: one upload.
    ///
    /// # Errors
    /// [`CaptureError::InvalidInput`] for an empty picture or too few bytes.
    pub fn place_pixels(
        &mut self,
        pixels: &[u8],
        width: u32,
        height: u32,
    ) -> Result<CanvasFrame, CaptureError> {
        check_pixels(pixels, width, height)?;
        let placed = CanvasPlacement::centered(self.size, (width, height));
        self.upload(pixels, width, placed)
    }

    /// Places the part of a `width` x `height` BGRA picture in memory that starts at `origin` at
    /// the canvas's top-left, as [`Self::place_texture_part`] does a texture: one upload.
    ///
    /// # Errors
    /// [`CaptureError::InvalidInput`] for an empty picture or too few bytes.
    pub fn place_pixels_part(
        &mut self,
        pixels: &[u8],
        width: u32,
        height: u32,
        origin: (u32, u32),
    ) -> Result<CanvasFrame, CaptureError> {
        check_pixels(pixels, width, height)?;
        let placed = CanvasPlacement::part(self.size, (width, height), origin);
        self.upload(pixels, width, placed)
    }

    /// Uploads `placed`'s part of a picture `width` wide (checked by `check_pixels`) onto the
    /// next slot.
    fn upload(
        &mut self,
        pixels: &[u8],
        width: u32,
        placed: CanvasPlacement,
    ) -> Result<CanvasFrame, CaptureError> {
        if placed.is_empty() {
            let slot = self.take_slot(Holds::Black);
            return self.frame(slot, placed);
        }
        let slot = self.take_slot(Holds::Picture(placed));
        let start = (placed.source_y as usize * width as usize + placed.source_x as usize) * 4;
        let area = D3D11_BOX {
            left: placed.x,
            top: placed.y,
            front: 0,
            right: placed.x + placed.width,
            bottom: placed.y + placed.height,
            back: 1,
        };
        // SAFETY: the slot is a live DEFAULT-usage texture; `area` is inside it; the source rows
        // start at `start` with a pitch of `width * 4` and the last row read ends within
        // `pixels` (`check_pixels` made sure it holds the whole picture, and `centered` and
        // `part` keep the area inside the picture past its offset).
        unsafe {
            self.context.UpdateSubresource(
                &self.slots[slot].texture,
                0,
                Some(&raw const area),
                pixels[start..].as_ptr().cast(),
                width * 4,
                0,
            );
        }
        self.frame(slot, placed)
    }

    /// A whole canvas of BGRA pixels (rows of `width * 4` bytes), said to hold the picture at
    /// `placed` (what [`CanvasFrame::placed`] reports): for a still made from
    /// [`Self::read_last`].
    ///
    /// # Errors
    /// [`CaptureError::InvalidInput`] for too few bytes.
    pub fn put_canvas(
        &mut self,
        pixels: &[u8],
        placed: CanvasPlacement,
    ) -> Result<CanvasFrame, CaptureError> {
        let row = self.size.0 as usize * 4;
        if pixels.len() < row * self.size.1 as usize {
            return Err(CaptureError::InvalidInput);
        }
        let slot = self.take_slot(Holds::Anything);
        // SAFETY: the slot is a live DEFAULT-usage texture of the canvas's size, and `pixels`
        // holds that many rows of `row` bytes (checked above).
        unsafe {
            self.context.UpdateSubresource(
                &self.slots[slot].texture,
                0,
                None,
                pixels.as_ptr().cast(),
                u32::try_from(row).map_err(|_| CaptureError::InvalidInput)?,
                0,
            );
        }
        self.frame(slot, placed)
    }

    /// The canvas last handed out, read back as BGRA rows of `width * 4` bytes; `None` before
    /// the first. Waits for the GPU, so it is for the occasional still, not every frame.
    ///
    /// # Errors
    /// [`CaptureError::Backend`] when the read-back texture cannot be made or mapped.
    pub fn read_last(&mut self) -> Result<Option<Vec<u8>>, CaptureError> {
        let Some(last) = self.last else {
            return Ok(None);
        };
        let staging = if let Some(staging) = &self.staging {
            // clone: COM AddRef of the cached staging texture
            staging.clone()
        } else {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: self.size.0,
                Height: self.size.1,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };
            let mut texture = None;
            // SAFETY: a valid staging description; no initial data.
            unsafe {
                self.device
                    .CreateTexture2D(&raw const desc, None, Some(&raw mut texture))
            }
            .map_err(|_| CaptureError::Backend)?;
            let texture = texture.ok_or(CaptureError::Backend)?;
            // clone: COM AddRef, cached for the next frame and used now
            self.staging = Some(texture.clone());
            texture
        };
        let row = self.size.0 as usize * 4;
        let rows = self.size.1 as usize;
        let mut out = vec![0u8; row * rows];
        // SAFETY: both textures are live, same size and format.
        unsafe {
            self.context
                .CopyResource(&staging, &self.slots[last].texture);
        }
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: a staging texture with CPU read access, mapped once and unmapped below.
        unsafe {
            self.context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&raw mut mapped))
        }
        .map_err(|_| CaptureError::Backend)?;
        let pitch = mapped.RowPitch as usize;
        if !mapped.pData.is_null() && pitch >= row {
            for (y, line) in out.chunks_exact_mut(row).enumerate() {
                // SAFETY: the mapping holds `rows` rows `pitch` bytes apart, each at least `row`
                // bytes long, valid until `Unmap`.
                let source = unsafe {
                    std::slice::from_raw_parts(mapped.pData.cast::<u8>().add(y * pitch), row)
                };
                line.copy_from_slice(source);
            }
        }
        // SAFETY: mapped above.
        unsafe { self.context.Unmap(&staging, 0) };
        Ok(Some(out))
    }

    /// The next slot, cleared to black first unless the new contents cover what it held.
    fn take_slot(&mut self, will_hold: Holds) -> usize {
        let index = self.next;
        self.next = (self.next + 1) % self.slots.len();
        let slot = &mut self.slots[index];
        let covered = match (slot.holds, will_hold) {
            (Holds::Black, _) | (_, Holds::Anything) => true,
            (Holds::Picture(old), Holds::Picture(new)) => new.covers(&old),
            (Holds::Anything | Holds::Picture(_), _) => false,
        };
        if !covered {
            // SAFETY: a live view on this device's context.
            unsafe { self.context.ClearRenderTargetView(&slot.view, &BLACK) };
        }
        slot.holds = will_hold;
        self.last = Some(index);
        index
    }

    fn frame(&self, slot: usize, placed: CanvasPlacement) -> Result<CanvasFrame, CaptureError> {
        let texture = &self.slots[slot].texture;
        let handle =
            NativeHandle::new(Interface::as_raw(texture) as usize).ok_or(CaptureError::Backend)?;
        Ok(CanvasFrame {
            frame: VideoFrame {
                pts: 0,
                duration: 1,
                width: self.size.0,
                height: self.size.1,
                format: PixelFormat::Bgra8,
                storage: VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 {
                    texture: handle,
                    subresource: 0,
                }),
            },
            placed,
        })
    }
}

/// Refuses an empty picture or one with fewer than `width * height * 4` bytes.
fn check_pixels(pixels: &[u8], width: u32, height: u32) -> Result<(), CaptureError> {
    let needed = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or(CaptureError::InvalidInput)?;
    if width == 0 || height == 0 || pixels.len() < needed {
        return Err(CaptureError::InvalidInput);
    }
    Ok(())
}

/// Opaque black.
const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

#[cfg(test)]
#[path = "canvas_tests.rs"]
mod tests;
