//! Windows Graphics Capture (WGC) for a single window — separate from DXGI screen.

#![allow(unsafe_code)]

use super::{CaptureBorder, FrameDimensions, WindowCaptureOptions};
use crate::CaptureError;
use crate::desktop::{
    CaptureOutputPreference, CaptureRegion, CursorCapture, DesktopCaptureSource,
    DesktopVideoCapture, DesktopVideoCaptureConfig,
};
use mediaway_common::{
    Bytes, CodecKind, GpuBufferHandle, GpuDeviceHandle, NativeHandle, PixelFormat, StreamInfo,
    VideoFrame, VideoFrameStorage, VideoGeometry,
};
use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureAccess,
    GraphicsCaptureAccessKind, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Security::Authorization::AppCapabilityAccess::AppCapabilityAccessStatus;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_TEXTURE2D_DESC, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread,
    ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
use windows::core::{Interface, factory};

struct HeldFrame {
    /// `None` when the delivered texture is a crop-ring slot: the WGC frame it was copied
    /// from has already gone back to the pool.
    _frame: Option<Direct3D11CaptureFrame>,
    _texture: ID3D11Texture2D,
}

/// How many crop-ring slots a region-copying session cycles through.
///
/// An encoder may still be reading a delivered texture after `release_frame`: the WMF
/// encoder wraps it in an `IMFSample` rather than copying it, and an async MFT processes
/// input later. The uncropped path already depends on that pipeline being shallow, because
/// WGC reuses its own 2-buffer pool the same way. Twice that depth keeps the crop path from
/// being the first to break if a pipeline turns out deeper than the pool.
const CROP_RING_DEPTH: usize = 4;

/// How a session turns WGC's frames into delivered frames.
enum Crop {
    /// The whole surface; the pool follows the content size (per `FrameDimensions`).
    None,
    /// A region at the surface origin: the pool is sized to it, and WGC clips the content
    /// into it. No copy.
    Pool { region: CaptureRegion },
    /// A region elsewhere: each frame's region is copied into a ring slot on the GPU.
    /// The ring is created on the first frame, from that frame's own texture description.
    Copy {
        region: CaptureRegion,
        ring: Option<CropRing>,
    },
}

struct CropRing {
    context: ID3D11DeviceContext,
    slots: [ID3D11Texture2D; CROP_RING_DEPTH],
    next: usize,
}

struct CaptureSession {
    device: ID3D11Device,
    // Recreate() needs a live IDirect3DDevice on every content-size change, so this is no
    // longer held purely for its lifetime side effect.
    winrt_device: IDirect3DDevice,
    _item: GraphicsCaptureItem,
    frame_pool: Direct3D11CaptureFramePool,
    _session: GraphicsCaptureSession,
    stream_info: StreamInfo,
    held: Option<HeldFrame>,
    next_pts: i64,
    border_hidden: bool,
    dimensions: FrameDimensions,
    crop: Crop,
}

/// Windows **window** capture via `WinRT` Graphics Capture (not DXGI Desktop Duplication).
pub struct WindowsWindowCapture {
    inner: Option<CaptureSession>,
}

impl WindowsWindowCapture {
    /// Open a WGC session for [`DesktopCaptureSource::Window`].
    ///
    /// Requires a live `HWND` (`config.source`) and caller `gpu_device` for Zero-Copy.
    /// Pair with process loopback audio when recording one app’s picture + sound.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Unsupported`] when WGC is unavailable or the source is not
    /// a window, or when the OS cannot apply `config.cursor` (see below). Returns
    /// [`CaptureError::InvalidInput`] for a null hwnd / unset device.
    ///
    /// # Cursor
    ///
    /// `config.cursor` is applied with `SetIsCursorCaptureEnabled` (Windows 10 2004+), and
    /// failing to apply it is an error **in both directions**. WGC's own default is to include
    /// the pointer. So on a build without that call, even an `Excluded` request would otherwise
    /// record a pointer nobody asked for.
    ///
    /// Uses [`WindowCaptureOptions::default`]: the border is left shown and frames are the
    /// window's exact size. See [`Self::open_with`].
    pub fn open(config: &DesktopVideoCaptureConfig) -> Result<Self, CaptureError> {
        Self::open_with(config, WindowCaptureOptions::default())
    }

    /// [`Self::open`] with Windows-specific options: the capture border, and whether frames
    /// are cropped to even dimensions for a hardware encoder.
    ///
    /// # Errors
    ///
    /// As [`Self::open`], plus [`CaptureError::Backend`] when
    /// [`FrameDimensions::EvenCropped`] would leave a zero axis (a 1-pixel window). A
    /// [`CaptureBorder::Hidden`] request the OS refuses is **not** an error — see
    /// [`CaptureBorder`] — and shows up as [`Self::border_hidden`] returning `false`.
    pub fn open_with(
        config: &DesktopVideoCaptureConfig,
        options: WindowCaptureOptions,
    ) -> Result<Self, CaptureError> {
        let DesktopCaptureSource::Window { window } = config.source else {
            return Err(CaptureError::Unsupported);
        };
        if config.output != CaptureOutputPreference::ZeroCopyGpu {
            return Err(CaptureError::Unsupported);
        }
        let Some(GpuDeviceHandle::DirectX11(handle)) = config.gpu_device else {
            return Err(CaptureError::InvalidInput);
        };
        if !GraphicsCaptureSession::IsSupported().unwrap_or(false) {
            return Err(CaptureError::Unsupported);
        }

        // SAFETY: WinRT apartment for Graphics Capture activation.
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };

        let raw = handle.get() as *mut std::ffi::c_void;
        // SAFETY: caller guarantees live ID3D11Device* for the session.
        let device_ref =
            unsafe { ID3D11Device::from_raw_borrowed(&raw) }.ok_or(CaptureError::InvalidInput)?;
        // clone: COM AddRef for session-owned device
        let device = device_ref.clone();

        let dxgi_device: IDXGIDevice = device.cast().map_err(|_| CaptureError::Backend)?;
        // SAFETY: WinRT wrapper around the same DXGI device.
        let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device) }
            .map_err(|_| CaptureError::Backend)?;
        let winrt_device: IDirect3DDevice =
            inspectable.cast().map_err(|_| CaptureError::Backend)?;

        let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
            .map_err(|_| CaptureError::Backend)?;
        let hwnd = HWND(window.get() as *mut _);
        // SAFETY: CreateForWindow requires a capturable top-level HWND.
        let item: GraphicsCaptureItem =
            unsafe { interop.CreateForWindow(hwnd) }.map_err(|_| CaptureError::AccessDenied)?;

        let item_size = item.Size().map_err(|_| CaptureError::Backend)?;
        let content_w = u32::try_from(item_size.Width).map_err(|_| CaptureError::Backend)?;
        let content_h = u32::try_from(item_size.Height).map_err(|_| CaptureError::Backend)?;
        let CropPlan {
            crop,
            frame: (width, height),
            pool: (pool_w, pool_h),
        } = plan_crop(config.region, options.dimensions, content_w, content_h)?;
        if matches!(crop, Crop::Copy { .. }) {
            // The region copy runs on the caller's thread while an encoder may use the same
            // device from its own. The WMF encoder turns this on as well; doing it here keeps
            // a crop-only caller safe too. Idempotent.
            if let Ok(mt) = device.cast::<ID3D11Multithread>() {
                // SAFETY: plain flag setter on a live device interface.
                let _ = unsafe { mt.SetMultithreadProtected(true) };
            }
        }

        let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &winrt_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size_int32(pool_w, pool_h)?,
        )
        .map_err(|_| CaptureError::Backend)?;

        let session = frame_pool
            .CreateCaptureSession(&item)
            .map_err(|_| CaptureError::Backend)?;
        session
            .SetIsCursorCaptureEnabled(config.cursor == CursorCapture::Included)
            .map_err(|_| CaptureError::Unsupported)?;
        let border_hidden = options.border == CaptureBorder::Hidden && hide_border(&session);
        session
            .StartCapture()
            .map_err(|_| CaptureError::AccessDenied)?;

        let stream_info = StreamInfo::Video {
            id: 0,
            codec: CodecKind::RawVideo,
            time_base: config.time_base,
            geometry: VideoGeometry { width, height },
            extra_data: Bytes::new(),
        };

        Ok(Self {
            inner: Some(CaptureSession {
                device,
                winrt_device,
                _item: item,
                frame_pool,
                _session: session,
                stream_info,
                held: None,
                next_pts: 0,
                border_hidden,
                dimensions: options.dimensions,
                crop,
            }),
        })
    }
}

impl WindowsWindowCapture {
    /// Whether the yellow capture border is actually hidden for this session.
    ///
    /// `false` when [`CaptureBorder::Shown`] was asked for, when the OS refused
    /// [`CaptureBorder::Hidden`], or once the session is closed. Read back from the session
    /// rather than remembered from the request, so it reports what Windows did.
    #[must_use]
    pub fn border_hidden(&self) -> bool {
        self.inner.as_ref().is_some_and(|s| s.border_hidden)
    }
}

/// Ask Windows not to draw the capture border for `session`. Returns whether it is now hidden.
///
/// Two steps, and either can refuse. The process has to be granted borderless capture
/// (`GraphicsCaptureAccess`), and the session then has to accept `IsBorderRequired = false`.
/// Measured 2026-09-18 on Windows 11 26100: an unpackaged desktop process is granted it
/// without a prompt (`AppCapabilityAccessStatus::Allowed`). A packaged app may instead see a
/// consent prompt, and this blocks until it is answered.
///
/// The answer is read back rather than assumed. Before this existed, a comment at the call
/// site said the border was hidden, and it never had been.
fn hide_border(session: &GraphicsCaptureSession) -> bool {
    let granted = GraphicsCaptureAccess::RequestAccessAsync(GraphicsCaptureAccessKind::Borderless)
        .and_then(|op| op.join())
        .is_ok_and(|status| status == AppCapabilityAccessStatus::Allowed);
    if !granted {
        return false;
    }
    if session.SetIsBorderRequired(false).is_err() {
        return false;
    }
    session.IsBorderRequired().is_ok_and(|required| !required)
}

impl DesktopVideoCapture for WindowsWindowCapture {
    fn stream_info(&self) -> &StreamInfo {
        #[allow(
            clippy::option_if_let_else,
            reason = "map_or_else forces 'static vs 'self lifetime clash"
        )]
        if let Some(s) = self.inner.as_ref() {
            &s.stream_info
        } else {
            closed_video_info()
        }
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, CaptureError> {
        let Some(session) = self.inner.as_mut() else {
            return Err(CaptureError::Closed);
        };
        if session.held.is_some() {
            return Err(CaptureError::Backend);
        }

        let Ok(frame) = session.frame_pool.TryGetNextFrame() else {
            return Ok(None);
        };

        let content = frame.ContentSize().map_err(|_| CaptureError::Backend)?;
        let Ok(content_w) = u32::try_from(content.Width) else {
            return Ok(None);
        };
        let Ok(content_h) = u32::try_from(content.Height) else {
            return Ok(None);
        };
        match &session.crop {
            Crop::None => {}
            // With a region the pool size is fixed by the region, not the window, so a resize
            // never needs a new pool. It only matters if the window no longer covers the
            // region, and then the frame would hold pixels WGC never drew there.
            Crop::Pool { region } | Crop::Copy { region, .. } => {
                if !region.fits_within(content_w, content_h) {
                    return Err(out_of_bounds(*region, content_w, content_h));
                }
                return deliver_region(session, frame);
            }
        }

        let current_geometry = session.stream_info.geometry().unwrap_or(VideoGeometry {
            width: 0,
            height: 0,
        });
        // Compared against the pool size the content *should* have, not the content size
        // itself. With `EvenCropped` an odd window never matches its own pool, so comparing
        // `ContentSize` directly would recreate the pool on every frame. (`ContentSize` keeps
        // reporting the full window when the pool is smaller — measured.)
        let Some((target_w, target_h)) = session.dimensions.pool_size(content_w, content_h) else {
            return Ok(None);
        };
        if let Some(new_geometry) = resized_geometry(current_geometry, target_w, target_h) {
            // Captured content size changed (window resized, or the captured monitor's mode
            // changed) — WGC requires recreating the frame pool at the new size so subsequent
            // buffers come back correctly sized; see `IDirect3D11CaptureFramePool::Recreate` in
            // Microsoft's WGC samples. The frame already in hand is still delivered below —
            // Stage 1 used to skip it forever instead, permanently stalling capture after a
            // resize. It came out of the *old* pool, so its size is read from its own texture.
            session
                .frame_pool
                .Recreate(
                    &session.winrt_device,
                    DirectXPixelFormat::B8G8R8A8UIntNormalized,
                    2,
                    size_int32(target_w, target_h)?,
                )
                .map_err(|_| CaptureError::Backend)?;
            let time_base = session.stream_info.time_base();
            session.stream_info = StreamInfo::Video {
                id: 0,
                codec: CodecKind::RawVideo,
                time_base,
                geometry: new_geometry,
                extra_data: Bytes::new(),
            };
        }

        let surface = frame.Surface().map_err(|_| CaptureError::Backend)?;
        let access: IDirect3DDxgiInterfaceAccess =
            surface.cast().map_err(|_| CaptureError::Backend)?;
        // SAFETY: WGC surface → ID3D11Texture2D via DXGI interop.
        let texture: ID3D11Texture2D =
            unsafe { access.GetInterface() }.map_err(|_| CaptureError::Backend)?;
        // The texture's own size, not the stream geometry: after a resize the frame in hand
        // is from the previous pool. This used to report the *new* geometry for it — a frame
        // whose stated size did not match its texture.
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: `texture` is a live ID3D11Texture2D; GetDesc only writes the out-param.
        unsafe { texture.GetDesc(&raw mut desc) };
        let texture_handle =
            NativeHandle::new(Interface::as_raw(&texture) as usize).ok_or(CaptureError::Backend)?;
        let pts = session.next_pts;
        session.next_pts = session.next_pts.saturating_add(1);
        session.held = Some(HeldFrame {
            _frame: Some(frame),
            _texture: texture,
        });

        Ok(Some(gpu_frame(pts, &desc, texture_handle)))
    }

    fn release_frame(&mut self) -> Result<(), CaptureError> {
        let Some(session) = self.inner.as_mut() else {
            return Err(CaptureError::Closed);
        };
        session.held = None;
        Ok(())
    }

    fn close(&mut self) -> Result<(), CaptureError> {
        if let Some(mut session) = self.inner.take() {
            session.held = None;
            let _ = session.frame_pool.Close();
        }
        Ok(())
    }
}

impl Drop for WindowsWindowCapture {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// What a session will do with each frame, the size it delivers, and the pool size WGC
/// should render into.
struct CropPlan {
    crop: Crop,
    /// Width and height of every delivered frame.
    frame: (u32, u32),
    /// Width and height of WGC's frame pool.
    pool: (u32, u32),
}

fn plan_crop(
    region: Option<CaptureRegion>,
    dimensions: FrameDimensions,
    content_w: u32,
    content_h: u32,
) -> Result<CropPlan, CaptureError> {
    let Some(requested) = region else {
        // The pool size *is* the crop: WGC clips content to the pool rather than scaling it
        // (measured — see `FrameDimensions`).
        let size = dimensions
            .pool_size(content_w, content_h)
            .ok_or(CaptureError::Backend)?;
        return Ok(CropPlan {
            crop: Crop::None,
            frame: size,
            pool: size,
        });
    };
    // `EvenCropped` applies to the region: it is what the encoder will see.
    let (w, h) = dimensions
        .pool_size(requested.width, requested.height)
        .ok_or(CaptureError::InvalidInput)?;
    let region = CaptureRegion {
        width: w,
        height: h,
        ..requested
    };
    if !region.fits_within(content_w, content_h) {
        return Err(out_of_bounds(region, content_w, content_h));
    }
    // The pool only has to reach the region's bottom-right corner: WGC clips content to the
    // pool from the top-left, so everything past that corner is work the compositor would do
    // for nothing. At the origin that corner *is* the region, and no copy is needed at all.
    let pool = (region.x + w, region.y + h);
    let crop = if region.is_at_origin() {
        Crop::Pool { region }
    } else {
        Crop::Copy { region, ring: None }
    };
    Ok(CropPlan {
        crop,
        frame: (w, h),
        pool,
    })
}

/// Deliver a region-cropped frame: WGC's texture as-is for an origin region (the pool already
/// clipped it), or a ring slot holding a GPU copy of the region otherwise.
fn deliver_region(
    session: &mut CaptureSession,
    frame: Direct3D11CaptureFrame,
) -> Result<Option<VideoFrame>, CaptureError> {
    let surface = frame.Surface().map_err(|_| CaptureError::Backend)?;
    let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(|_| CaptureError::Backend)?;
    // SAFETY: WGC surface → ID3D11Texture2D via DXGI interop.
    let source: ID3D11Texture2D =
        unsafe { access.GetInterface() }.map_err(|_| CaptureError::Backend)?;
    let pts = session.next_pts;
    session.next_pts = session.next_pts.saturating_add(1);

    let (texture, keep_frame) = match &mut session.crop {
        Crop::None | Crop::Pool { .. } => (source, true),
        Crop::Copy { region, ring } => {
            let region = *region;
            if ring.is_none() {
                *ring = Some(create_crop_ring(&session.device, &source, region)?);
            }
            let Some(ring) = ring.as_mut() else {
                return Err(CaptureError::Backend);
            };
            // clone: COM AddRef — the slot stays in the ring and is also held for the caller.
            let slot = ring.slots[ring.next].clone();
            ring.next = (ring.next + 1) % CROP_RING_DEPTH;
            let area = D3D11_BOX {
                left: region.x,
                top: region.y,
                front: 0,
                right: region.x + region.width,
                bottom: region.y + region.height,
                back: 1,
            };
            // SAFETY: both textures are live on the same device, share a format, and `area`
            // lies inside the source (the pool reaches the region's bottom-right corner) and
            // matches the slot's size exactly.
            unsafe {
                ring.context.CopySubresourceRegion(
                    &slot,
                    0,
                    0,
                    0,
                    0,
                    &source,
                    0,
                    Some(&raw const area),
                );
            }
            (slot, false)
        }
    };

    let mut desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: `texture` is a live ID3D11Texture2D; GetDesc only writes the out-param.
    unsafe { texture.GetDesc(&raw mut desc) };
    let handle =
        NativeHandle::new(Interface::as_raw(&texture) as usize).ok_or(CaptureError::Backend)?;
    // A copied region no longer needs WGC's buffer, so it goes back to the pool now rather
    // than at `release_frame`.
    session.held = Some(HeldFrame {
        _frame: keep_frame.then_some(frame),
        _texture: texture,
    });
    Ok(Some(gpu_frame(pts, &desc, handle)))
}

/// The ring a region-copying session cycles through: `CROP_RING_DEPTH` textures shaped like
/// WGC's own (same format, bind flags and usage — which the encoder already accepts) but the
/// size of the region.
fn create_crop_ring(
    device: &ID3D11Device,
    source: &ID3D11Texture2D,
    region: CaptureRegion,
) -> Result<CropRing, CaptureError> {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: `source` is a live ID3D11Texture2D; GetDesc only writes the out-param.
    unsafe { source.GetDesc(&raw mut desc) };
    desc.Width = region.width;
    desc.Height = region.height;
    desc.MipLevels = 1;
    desc.ArraySize = 1;
    // SAFETY: plain getter on a live device.
    let context = unsafe { device.GetImmediateContext() }.map_err(|_| CaptureError::Backend)?;
    let make = || -> Result<ID3D11Texture2D, CaptureError> {
        let mut texture = None;
        // SAFETY: `desc` is a valid, fully initialised description; no initial data.
        unsafe { device.CreateTexture2D(&raw const desc, None, Some(&raw mut texture)) }
            .map_err(|_| CaptureError::Backend)?;
        texture.ok_or(CaptureError::Backend)
    };
    let slots = [make()?, make()?, make()?, make()?];
    Ok(CropRing {
        context,
        slots,
        next: 0,
    })
}

const fn gpu_frame(pts: i64, desc: &D3D11_TEXTURE2D_DESC, texture: NativeHandle) -> VideoFrame {
    VideoFrame {
        pts,
        duration: 1,
        width: desc.Width,
        height: desc.Height,
        format: PixelFormat::Bgra8,
        storage: VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 {
            texture,
            subresource: 0,
        }),
    }
}

const fn out_of_bounds(region: CaptureRegion, width: u32, height: u32) -> CaptureError {
    CaptureError::RegionOutOfBounds {
        x: region.x,
        y: region.y,
        width: region.width,
        height: region.height,
        surface_width: width,
        surface_height: height,
    }
}

/// A frame-pool size as `WinRT` wants it. The inputs came from `i32`s `WinRT` gave us, so
/// this only fails if a caller hands in something `WinRT` never could have.
fn size_int32(width: u32, height: u32) -> Result<SizeInt32, CaptureError> {
    Ok(SizeInt32 {
        Width: i32::try_from(width).map_err(|_| CaptureError::Backend)?,
        Height: i32::try_from(height).map_err(|_| CaptureError::Backend)?,
    })
}

fn closed_video_info() -> &'static StreamInfo {
    use std::sync::OnceLock;
    static INFO: OnceLock<StreamInfo> = OnceLock::new();
    INFO.get_or_init(|| StreamInfo::Video {
        id: 0,
        codec: CodecKind::RawVideo,
        time_base: mediaway_common::Rational::new(1, 30),
        geometry: VideoGeometry {
            width: 0,
            height: 0,
        },
        extra_data: Bytes::new(),
    })
}

/// Returns the new geometry when a just-arrived frame's `ContentSize` no longer matches
/// `current`, signaling the caller to `Direct3D11CaptureFramePool::Recreate` at the new
/// size. `None` means the frame pool is still sized correctly.
///
/// Pure comparison — no `WinRT` calls — so the resize-detection decision is unit-testable
/// without a live WGC session (driving an actual window resize is not practically
/// automatable in this test suite).
const fn resized_geometry(
    current: VideoGeometry,
    content_width: u32,
    content_height: u32,
) -> Option<VideoGeometry> {
    if content_width == current.width && content_height == current.height {
        None
    } else {
        Some(VideoGeometry {
            width: content_width,
            height: content_height,
        })
    }
}

#[cfg(test)]
#[path = "wgc_tests.rs"]
mod tests;
