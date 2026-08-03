//! Windows Graphics Capture (WGC) for a single window — separate from DXGI screen.

#![allow(unsafe_code)]

use crate::CaptureError;
use crate::desktop::{
    CaptureOutputPreference, DesktopCaptureSource, DesktopVideoCapture, DesktopVideoCaptureConfig,
};
use mediaway_common::{
    Bytes, CodecKind, GpuBufferHandle, GpuDeviceHandle, NativeHandle, PixelFormat, StreamInfo,
    VideoFrame, VideoFrameStorage, VideoGeometry,
};
use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
use windows::core::{Interface, factory};

struct HeldFrame {
    _frame: Direct3D11CaptureFrame,
    _texture: ID3D11Texture2D,
}

struct CaptureSession {
    _device: ID3D11Device,
    // Recreate() needs a live IDirect3DDevice on every content-size change, so this is no
    // longer held purely for its lifetime side effect.
    winrt_device: IDirect3DDevice,
    _item: GraphicsCaptureItem,
    frame_pool: Direct3D11CaptureFramePool,
    _session: GraphicsCaptureSession,
    stream_info: StreamInfo,
    held: Option<HeldFrame>,
    next_pts: i64,
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
    /// a window. Returns [`CaptureError::InvalidInput`] for a null hwnd / unset device.
    pub fn open(config: &DesktopVideoCaptureConfig) -> Result<Self, CaptureError> {
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

        let size = item.Size().map_err(|_| CaptureError::Backend)?;
        if size.Width <= 0 || size.Height <= 0 {
            return Err(CaptureError::Backend);
        }
        let width = u32::try_from(size.Width).map_err(|_| CaptureError::Backend)?;
        let height = u32::try_from(size.Height).map_err(|_| CaptureError::Backend)?;

        let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &winrt_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size,
        )
        .map_err(|_| CaptureError::Backend)?;

        let session = frame_pool
            .CreateCaptureSession(&item)
            .map_err(|_| CaptureError::Backend)?;
        // Best-effort: hide yellow border / cursor when the OS build supports it.
        let _ = session.SetIsCursorCaptureEnabled(false);
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
                _device: device,
                winrt_device,
                _item: item,
                frame_pool,
                _session: session,
                stream_info,
                held: None,
                next_pts: 0,
            }),
        })
    }
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
        let current_geometry = session.stream_info.geometry().unwrap_or(VideoGeometry {
            width: 0,
            height: 0,
        });
        let geometry =
            if let Some(new_geometry) = resized_geometry(current_geometry, content_w, content_h) {
                // Captured content size changed (window resized, or the captured monitor's
                // mode changed) — WGC requires recreating the frame pool at the new size so
                // subsequent buffers come back correctly sized; see
                // `IDirect3D11CaptureFramePool::Recreate` in Microsoft's WGC samples. The
                // frame already in hand is still delivered below, sized to its own
                // `ContentSize` (not the stale, pre-resize geometry) — Stage 1 used to skip
                // it forever instead, permanently stalling capture after a resize.
                session
                    .frame_pool
                    .Recreate(
                        &session.winrt_device,
                        DirectXPixelFormat::B8G8R8A8UIntNormalized,
                        2,
                        content,
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
                new_geometry
            } else {
                current_geometry
            };

        let surface = frame.Surface().map_err(|_| CaptureError::Backend)?;
        let access: IDirect3DDxgiInterfaceAccess =
            surface.cast().map_err(|_| CaptureError::Backend)?;
        // SAFETY: WGC surface → ID3D11Texture2D via DXGI interop.
        let texture: ID3D11Texture2D =
            unsafe { access.GetInterface() }.map_err(|_| CaptureError::Backend)?;
        let texture_handle =
            NativeHandle::new(Interface::as_raw(&texture) as usize).ok_or(CaptureError::Backend)?;
        let pts = session.next_pts;
        session.next_pts = session.next_pts.saturating_add(1);
        session.held = Some(HeldFrame {
            _frame: frame,
            _texture: texture,
        });

        Ok(Some(VideoFrame {
            pts,
            duration: 1,
            width: geometry.width,
            height: geometry.height,
            format: PixelFormat::Bgra8,
            storage: VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 {
                texture: texture_handle,
                subresource: 0,
            }),
        }))
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
