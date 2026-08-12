//! Shared `CMSampleBuffer` → `CVPixelBuffer` CPU readback routine — reused by `camera.rs`,
//! `screencapturekit.rs`, and `replaykit.rs` (one extraction routine, not three independent
//! implementations, per adr/apple/0001 § Frame extraction and adr/apple/0003's reuse note).
//!
//! The capture-side mirror of `mediaway-encoder::apple`'s `upload_cpu_nv12` (readback **from** a
//! `CVPixelBuffer`, not upload **to** one) — named per `docs/spec/caveats-and-clarity.md`'s
//! honest-cost-naming rule.

#![allow(unsafe_code)]

use mediaway_common::Bytes;
use objc2_core_media::CMSampleBuffer;
use objc2_core_video::{CVPixelBuffer, CVPixelBufferLockFlags};

/// Extract one owned NV12-shaped (bi-planar 4:2:0) frame from `sample_buffer`'s image buffer.
/// Returns `None` if the buffer has no image, does not downcast to `CVPixelBuffer`, or is not
/// bi-planar (never silently mis-reads a layout it didn't verify).
///
/// # Safety
///
/// `sample_buffer` must be a valid, non-null `CMSampleBuffer` for the duration of this call —
/// the caller's own delegate/callback contract (Apple guarantees this for the duration of a
/// synchronous sample-buffer delivery callback).
pub(super) unsafe fn extract_nv12(sample_buffer: &CMSampleBuffer) -> Option<(Bytes, u32, u32)> {
    // SAFETY: caller's contract (this fn's own `# Safety`).
    let image_buffer = unsafe { sample_buffer.image_buffer() }?;
    let pixel_buffer = image_buffer.downcast::<CVPixelBuffer>().ok()?;

    // SAFETY: `pixel_buffer` is a valid, retained `CVPixelBuffer`.
    let status = unsafe { pixel_buffer.lock_base_address(CVPixelBufferLockFlags::ReadOnly) };
    if status != 0 {
        return None;
    }

    let result = (|| {
        let plane_count = pixel_buffer.plane_count();
        if plane_count != 2 {
            return None;
        }
        let width = u32::try_from(pixel_buffer.width_of_plane(0)).ok()?;
        let height = u32::try_from(pixel_buffer.height_of_plane(0)).ok()?;
        let mut out = Vec::new();
        for plane in 0..2 {
            let plane_width = pixel_buffer.width_of_plane(plane);
            let plane_height = pixel_buffer.height_of_plane(plane);
            let row_bytes = pixel_buffer.bytes_per_row_of_plane(plane);
            let row_width_bytes = if plane == 0 {
                plane_width
            } else {
                plane_width * 2
            };
            let base = pixel_buffer.base_address_of_plane(plane);
            if base.is_null() || row_bytes < row_width_bytes {
                return None;
            }
            for row in 0..plane_height {
                // SAFETY: `base` is a valid, locked plane base address (checked non-null
                // above); `row < plane_height` and `row_bytes`/`row_width_bytes` were read from
                // the same buffer, so `row * row_bytes + row_width_bytes` stays within the
                // plane's real allocation per `CVPixelBuffer`'s own row-stride/plane-height
                // contract.
                let row_ptr = unsafe { base.cast::<u8>().add(row * row_bytes) };
                // SAFETY: same contract — `row_ptr` is readable for `row_width_bytes` bytes.
                let row_slice = unsafe { std::slice::from_raw_parts(row_ptr, row_width_bytes) };
                out.extend_from_slice(row_slice);
            }
        }
        Some((Bytes::from(out), width, height))
    })();

    // SAFETY: `pixel_buffer` is the same, still-valid buffer locked above.
    unsafe { pixel_buffer.unlock_base_address(CVPixelBufferLockFlags::ReadOnly) };
    result
}
