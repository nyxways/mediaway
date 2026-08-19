//! Codec-agnostic VA-API DMA-BUF import: caller-supplied
//! [`mediaway_common::GpuBufferHandle::DmaBuf`] → `DmaBufImportDescriptor`
//! (`cros_libva::ExternalBufferDescriptor`) → a single imported `Surface`.
//!
//! See [ADR-0006](../../../adr/linux/0006-vaapi-dmabuf-zero-copy-input.md) for the design this
//! module implements — in particular § "`cros-libva` exposes an import-side trait" and § fd
//! ownership. Mirrors `mediaway-decoder`'s companion `dmabuf.rs` module (same underlying
//! mechanism, opposite direction) but is not shared with it — see that ADR's Alternatives
//! Considered for why (no dependency relationship between the two crates, and the `cros-libva`
//! call shapes genuinely differ: `Surface::export_prime()` vs. implementing
//! `ExternalBufferDescriptor` + `Display::create_surfaces`).
//!
//! No codec-specific types anywhere in this file — reusable by a future `hevc.rs`/`vp9.rs`
//! VA-API encoder backend in this crate for free.

use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd, RawFd};
use std::rc::Rc;

use cros_libva::{
    Display, ExternalBufferDescriptor, MemoryType, Surface, UsageHint, VA_FOURCC_NV12,
    VA_RT_FORMAT_YUV420, VADRMPRIMESurfaceDescriptor,
};
use mediaway_common::{DmaBufDescriptor, NativeHandle};

use crate::EncodeError;

/// Local descriptor implementing `cros_libva::ExternalBufferDescriptor`, built from a caller's
/// [`DmaBufDescriptor`]. Holds this backend's own defensive `dup()` of the caller's fd(s) —
/// closed by [`import_surface`] right after `Display::create_surfaces` returns (see the owning
/// ADR's § fd ownership / Open questions #1); `None` afterward, since VA-API does not need the
/// fd(s) to stay open past that call under this ADR's (unconfirmed, defensively-assumed) model
/// of `vaCreateSurfaces`'s import semantics.
pub(crate) struct DmaBufImportDescriptor {
    fourcc: u32,
    width: u32,
    height: u32,
    modifier: u64,
    fd0: Option<OwnedFd>,
    fd1: Option<OwnedFd>,
    planes: [mediaway_common::DmaBufPlane; 2],
    plane_count: u8,
}

impl ExternalBufferDescriptor for DmaBufImportDescriptor {
    const MEMORY_TYPE: MemoryType = MemoryType::DrmPrime2;
    type DescriptorAttribute = VADRMPRIMESurfaceDescriptor;

    fn va_surface_attribute(&mut self) -> Self::DescriptorAttribute {
        let mut desc = VADRMPRIMESurfaceDescriptor {
            fourcc: self.fourcc,
            width: self.width,
            height: self.height,
            num_objects: u32::from(self.fd1.is_some()) + 1,
            num_layers: 1,
            ..Default::default()
        };
        if let Some(fd0) = &self.fd0 {
            desc.objects[0].fd = fd0.as_raw_fd();
            desc.objects[0].drm_format_modifier = self.modifier;
        }
        if let Some(fd1) = &self.fd1 {
            desc.objects[1].fd = fd1.as_raw_fd();
            desc.objects[1].drm_format_modifier = self.modifier;
        }
        desc.layers[0].drm_format = self.fourcc;
        desc.layers[0].num_planes = u32::from(self.plane_count);
        for i in 0..usize::from(self.plane_count) {
            let plane = self.planes[i];
            desc.layers[0].object_index[i] = u32::from(plane.object_index);
            desc.layers[0].offset[i] = plane.offset;
            desc.layers[0].pitch[i] = plane.pitch;
        }
        desc
    }
}

/// Import `desc` as a single-use VA-API surface for this call's encode input.
///
/// Not pooled or recycled by this backend — see the owning ADR's § "Why no
/// `outstanding`/lifetime bookkeeping is needed here": the returned surface is used once by the
/// caller's `encode_one` and dropped, never held across `push_frame` calls.
///
/// # Errors
///
/// Returns [`EncodeError::InvalidInput`] if `desc`'s plane/object shape or `fourcc` does not
/// match this backend's NV12 expectation (never silently reinterpreted), or
/// [`EncodeError::Backend`] on `vaCreateSurfaces` failure.
pub(crate) fn import_surface(
    display: &Rc<Display>,
    desc: &DmaBufDescriptor,
    width: u32,
    height: u32,
) -> Result<Surface<DmaBufImportDescriptor>, EncodeError> {
    if desc.fourcc != VA_FOURCC_NV12 {
        // ADR-0003 (decoder companion) Open questions #2: VA_FOURCC_NV12 and DRM_FORMAT_NV12
        // are assumed numerically identical for NV12 — not independently re-verified here.
        return Err(EncodeError::InvalidInput);
    }
    if desc.plane_count == 0 || desc.plane_count > 2 {
        return Err(EncodeError::InvalidInput);
    }

    let fd0 = dup_from_native(desc.fd0)?;
    let fd1 = desc.fd1.map(dup_from_native).transpose()?;

    let descriptor = DmaBufImportDescriptor {
        fourcc: desc.fourcc,
        width: desc.width,
        height: desc.height,
        modifier: desc.modifier,
        fd0: Some(fd0),
        fd1,
        planes: desc.planes,
        plane_count: desc.plane_count,
    };

    let mut surfaces = display
        .create_surfaces(
            VA_RT_FORMAT_YUV420,
            Some(VA_FOURCC_NV12),
            width,
            height,
            Some(UsageHint::USAGE_HINT_ENCODER),
            vec![descriptor],
        )
        .map_err(|_| EncodeError::Backend)?;
    let mut surface = surfaces.pop().ok_or(EncodeError::Backend)?;

    // Close our defensive dup(s) now that `vaCreateSurfaces` has (per this ADR's assumed model)
    // already consumed them — see the module/struct doc above. On the error path above, the
    // `Vec<DmaBufImportDescriptor>` (and thus its `OwnedFd`s) was already dropped internally by
    // `Display::create_surfaces`'s own failure path — no separate cleanup needed there.
    let held = surface.as_mut();
    held.fd0 = None;
    held.fd1 = None;

    Ok(surface)
}

/// Reconstructs the raw fd `handle` names (per [`DmaBufDescriptor::fd0`]'s documented `+1`
/// offset convention) and returns this backend's own independent duplicate of it.
///
/// # Errors
///
/// Returns [`EncodeError::InvalidInput`] if `handle`'s bits do not decode to a valid fd number,
/// or [`EncodeError::Backend`] if the duplicate syscall fails.
fn dup_from_native(handle: NativeHandle) -> Result<OwnedFd, EncodeError> {
    let bits = handle
        .get()
        .checked_sub(1)
        .ok_or(EncodeError::InvalidInput)?;
    let raw = RawFd::try_from(bits).map_err(|_| EncodeError::InvalidInput)?;
    // SAFETY: `raw` is decoded from a caller-supplied `GpuBufferHandle::DmaBuf` — per that
    // type's documented contract (`mediaway_common::gpu::DmaBufDescriptor::fd0`), it names a
    // real, currently-open DMA-BUF fd owned by the frame's producer for at least the duration of
    // this synchronous `push_frame` call. This `BorrowedFd` never outlives this function and is
    // never used to close or otherwise take ownership of `raw` — only to duplicate it below.
    let borrowed = unsafe { BorrowedFd::borrow_raw(raw) };
    borrowed
        .try_clone_to_owned()
        .map_err(|_| EncodeError::Backend)
}

#[cfg(test)]
#[path = "dmabuf_tests.rs"]
mod tests;
