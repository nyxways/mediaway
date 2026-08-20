//! Opt-in exclusive (non-shared) DXGI Desktop Duplication — true Zero-Copy.
//!
//! See [ADR-0008](../adr/windows/0008-exclusive-desktop-duplication-zero-copy.md). No driver
//! thread, no ring, no `CopyResource`: [`ExclusiveDuplication::poll_frame`]/[`release_frame`](ExclusiveDuplication::release_frame)
//! call `AcquireNextFrame`/`ReleaseFrame` directly on the calling thread and hand out the
//! DDA-owned texture itself. The caller asserts it is the only consumer for this output — see
//! [`crate::desktop::CaptureSharing::Exclusive`]'s own docs for the concurrent-open failure mode
//! (enforced by DXGI itself, not this module).
//!
//! **Lifetime caveat**: the returned frame's texture is the DDA's real backing resource,
//! invalidated the instant `ReleaseFrame` runs — a caller must finish *issuing* (not
//! necessarily completing) any GPU work that reads it before calling [`ExclusiveDuplication::release_frame`],
//! exactly the "issue-then-drop, never drop-then-issue-later" contract the shared ring
//! (`dxgi_shared.rs`) already documents for its own recycle signal.

#![allow(unsafe_code)]
// `dxgi_exclusive` is a private module (`mod dxgi_exclusive;`, not `pub mod`), so
// `pub(crate)` items here are only ever crate-reachable either way — same
// `redundant_pub_crate` tension `dxgi_shared.rs` already documents for its own
// `pub(crate)` items.
#![allow(clippy::redundant_pub_crate)]

use crate::CaptureError;
use mediaway_common::{
    Bytes, CodecKind, GpuBufferHandle, NativeHandle, PixelFormat, StreamInfo, VideoFrame,
    VideoFrameStorage, VideoGeometry,
};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};
use windows::Win32::Graphics::Dxgi::{
    DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, IDXGIDevice, IDXGIOutput1,
    IDXGIOutputDuplication, IDXGIResource,
};
use windows::core::Interface;

const POLL_TIMEOUT_MS: u32 = 16;

/// A single-consumer DXGI Desktop Duplication session — no driver thread, no shared state.
pub(crate) struct ExclusiveDuplication {
    duplication: IDXGIOutputDuplication,
    /// `Some` between [`Self::poll_frame`] and [`Self::release_frame`] — the live DDA-owned
    /// texture. Its raw pointer is what `poll_frame` hands out directly (never copied).
    held: Option<ID3D11Texture2D>,
}

impl ExclusiveDuplication {
    /// Open a real, exclusive `IDXGIOutputDuplication` on `output_index` via `device_raw`.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::InvalidInput`] for a null device pointer or an out-of-range
    /// `output_index`. Returns [`CaptureError::AccessDenied`] when `DuplicateOutput` fails —
    /// including the case a `Shared` or another `Exclusive` session is already live for this
    /// output (DXGI allows only one live duplication per output per process). Returns
    /// [`CaptureError::Backend`] on other DXGI failures.
    pub(crate) fn open(
        device_raw: usize,
        output_index: u32,
    ) -> Result<(Self, StreamInfo), CaptureError> {
        let raw = device_raw as *mut std::ffi::c_void;
        // SAFETY: caller guarantees a live `ID3D11Device*` for the session.
        let device_ref =
            unsafe { ID3D11Device::from_raw_borrowed(&raw) }.ok_or(CaptureError::InvalidInput)?;
        // clone: COM AddRef so this session owns a reference for its own lifetime.
        let device: ID3D11Device = device_ref.clone();

        let dxgi_device: IDXGIDevice = device.cast().map_err(|_| CaptureError::Backend)?;
        // SAFETY: GetAdapter is a proven, compiling precedent (see `dxgi_shared.rs::open_duplication`).
        let adapter = unsafe { dxgi_device.GetAdapter() }.map_err(|_| CaptureError::Backend)?;
        // SAFETY: EnumOutputs is a DXGI adapter query with no retained pointers.
        let output =
            unsafe { adapter.EnumOutputs(output_index) }.map_err(|_| CaptureError::InvalidInput)?;
        let output1: IDXGIOutput1 = output.cast().map_err(|_| CaptureError::Backend)?;
        // SAFETY: DuplicateOutput on this caller's own device. Fails with a real DXGI error
        // (mapped to AccessDenied below) if another duplication is already live for this
        // output — the correctness backstop `CaptureSharing::Exclusive`'s docs promise.
        let duplication =
            unsafe { output1.DuplicateOutput(&device) }.map_err(|_| CaptureError::AccessDenied)?;

        // SAFETY: GetDesc reads a fixed-size struct with no retained pointers.
        let dup_desc = unsafe { duplication.GetDesc() };
        let width = dup_desc.ModeDesc.Width;
        let height = dup_desc.ModeDesc.Height;
        if width == 0 || height == 0 {
            return Err(CaptureError::Backend);
        }

        let stream_info = StreamInfo::Video {
            id: 0,
            codec: CodecKind::RawVideo,
            time_base: mediaway_common::Rational::new(1, 60),
            geometry: VideoGeometry { width, height },
            extra_data: Bytes::new(),
        };

        Ok((
            Self {
                duplication,
                held: None,
            },
            stream_info,
        ))
    }

    /// Pull the next frame if ready — no copy, the returned handle is the DDA's own texture.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Backend`] if a frame is already held (must
    /// [`Self::release_frame`] first — mirrors `dxgi_shared`'s `ConsumerRecord.held` check) or
    /// on a DXGI failure other than a timeout (`Ok(None)`).
    pub(crate) fn poll_frame(
        &mut self,
        geometry: VideoGeometry,
        next_pts: &mut i64,
    ) -> Result<Option<VideoFrame>, CaptureError> {
        if self.held.is_some() {
            return Err(CaptureError::Backend);
        }

        let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut desktop_resource: Option<IDXGIResource> = None;
        // SAFETY: this struct is never shared across threads while a poll/release pair is in
        // flight (caller's own discipline, same as every other `DesktopVideoCapture` backend);
        // no separate driver thread exists here to race against.
        let acquire = unsafe {
            self.duplication.AcquireNextFrame(
                POLL_TIMEOUT_MS,
                &raw mut frame_info,
                &raw mut desktop_resource,
            )
        };
        if let Err(e) = acquire {
            if e.code() == DXGI_ERROR_WAIT_TIMEOUT {
                return Ok(None);
            }
            return Err(CaptureError::Backend);
        }

        let Some(desktop_resource) = desktop_resource else {
            return Ok(None);
        };
        let Ok(texture) = desktop_resource.cast::<ID3D11Texture2D>() else {
            // SAFETY: release the frame we failed to cast before returning.
            let _ = unsafe { self.duplication.ReleaseFrame() };
            return Err(CaptureError::Backend);
        };
        let raw_texture_ptr = Interface::as_raw(&texture) as usize;
        self.held = Some(texture);

        let texture_handle = NativeHandle::new(raw_texture_ptr).ok_or(CaptureError::Backend)?;
        let pts = *next_pts;
        *next_pts += 1;
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

    /// Release the held frame — calls `ReleaseFrame`, invalidating the texture
    /// [`Self::poll_frame`] returned. No-op if nothing is held.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Backend`] if `ReleaseFrame` itself fails.
    pub(crate) fn release_frame(&mut self) -> Result<(), CaptureError> {
        if self.held.take().is_some() {
            // SAFETY: pairs with the `AcquireNextFrame` in `poll_frame`.
            unsafe { self.duplication.ReleaseFrame() }.map_err(|_| CaptureError::Backend)?;
        }
        Ok(())
    }
}

impl Drop for ExclusiveDuplication {
    fn drop(&mut self) {
        let _ = self.release_frame();
    }
}
