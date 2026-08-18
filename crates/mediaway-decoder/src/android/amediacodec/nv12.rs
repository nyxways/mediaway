//! Strip `AMediaCodec` `stride`/`slice-height` padding and apply the negotiated crop rect to
//! produce a tightly packed NV12 [`Bytes`] buffer.
//!
//! Pure byte-copy logic, independent of a real `MediaCodec`/`AMediaFormat` session so it is
//! unit-testable without an Android device or NDK — the caller ([`super::video`]) supplies
//! `stride`/`slice_height`/crop read from a real `AMediaFormat` output format negotiated after
//! an `OutputFormatChanged` event. Output layout matches this crate's other NV12 CPU paths
//! (`linux::vaapi::nv12`, `windows::wmf::cpu`): tightly packed luma plane followed by tightly
//! packed interleaved chroma plane, stride removed — the Android-specific analog of
//! `linux::vaapi::nv12`'s pitch-stripping (stride/slice-height/crop keys vs. `VAImage`'s
//! `pitches[]`/`offsets[]`), same purpose, same "genuine driver→CPU copy" honesty note (see
//! ADR android/0001 § Decision).

use mediaway_common::Bytes;

/// `AMediaFormat` crop rectangle (`"crop-left"`/`"crop-top"`/`"crop-right"`/`"crop-bottom"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CropRect {
    pub(super) left: u32,
    pub(super) top: u32,
    pub(super) right: u32,
    pub(super) bottom: u32,
}

impl CropRect {
    /// Cropped output width. Per ADR android/0001 § Decision: `right - left` (no `+1`) — this
    /// crate's own convention for this backend, mirroring `windows::wmf::video_cpu`'s
    /// `apply_stream_change` geometry update, not necessarily every device's inclusive-bound
    /// reading of the same keys.
    pub(super) const fn width(self) -> u32 {
        self.right.saturating_sub(self.left)
    }

    /// Cropped output height (`bottom - top`).
    pub(super) const fn height(self) -> u32 {
        self.bottom.saturating_sub(self.top)
    }
}

/// Copy an `AMediaCodec` `COLOR_FormatYUV420SemiPlanar` output buffer's `stride`/
/// `slice_height`-padded planes into a tightly packed, cropped NV12 [`Bytes`] buffer.
///
/// `data` is the output buffer's valid payload region (`BufferInfo::offset()`..`+size()`
/// already applied by the caller). `stride` is the luma/chroma row pitch in bytes
/// (`"stride"`), `slice_height` the number of luma rows before the chroma plane starts
/// (`"slice-height"`, or `height` when the device omits the key / reports `0` — the documented
/// zero-means-"same as height" quirk, resolved by the caller before this function runs).
/// `crop` is read straight from `"crop-left"`/`"crop-top"`/`"crop-right"`/`"crop-bottom"`.
///
/// Rows/columns that would read past `data`'s end are left zeroed (defensive: a device
/// reporting inconsistent stride/slice-height/crop should not panic this path, same discipline
/// as `linux::vaapi::nv12::copy_nv12_from_planes`).
pub(super) fn strip_and_crop_nv12(
    data: &[u8],
    stride: u32,
    slice_height: u32,
    crop: CropRect,
) -> Bytes {
    let out_width = crop.width() as usize;
    let out_height = crop.height() as usize;
    if out_width == 0 || out_height == 0 {
        return Bytes::new();
    }
    let stride = stride as usize;
    let left = crop.left as usize;
    let top = crop.top as usize;
    let uv_plane_offset = stride * slice_height as usize;
    let uv_rows = out_height / 2;

    let mut out = vec![0u8; out_width * out_height + out_width * uv_rows];

    for row in 0..out_height {
        let Some(src_start) = top
            .checked_add(row)
            .and_then(|r| r.checked_mul(stride))
            .and_then(|base| base.checked_add(left))
        else {
            break;
        };
        let src_end = src_start + out_width;
        if src_end > data.len() {
            break;
        }
        let dst_start = row * out_width;
        out[dst_start..dst_start + out_width].copy_from_slice(&data[src_start..src_end]);
    }

    let y_plane_bytes = out_width * out_height;
    let chroma_top = top / 2;
    for row in 0..uv_rows {
        let Some(src_start) = chroma_top
            .checked_add(row)
            .and_then(|r| r.checked_mul(stride))
            .and_then(|base| base.checked_add(uv_plane_offset))
            .and_then(|base| base.checked_add(left))
        else {
            break;
        };
        let src_end = src_start + out_width;
        if src_end > data.len() {
            break;
        }
        let dst_start = y_plane_bytes + row * out_width;
        out[dst_start..dst_start + out_width].copy_from_slice(&data[src_start..src_end]);
    }

    Bytes::from(out)
}

#[cfg(test)]
#[path = "nv12_tests.rs"]
mod tests;
