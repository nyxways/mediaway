//! CPU (software) H.264 decode: no DXGI device manager, no GPU dependency.
//!
//! [`VideoOutputPreference::CpuFramesOk`](mediaway_decoder::VideoOutputPreference) opens a
//! synchronous software H.264 decoder MFT and reads NV12 planes directly out of its
//! system-memory output buffer — no HW MFT, no `ID3D11Device`, and no GPU→CPU readback
//! (there is no GPU texture in this path at all).

#![allow(unsafe_code)]

use mediaway_common::Bytes;
use mediaway_decoder::DecodeError;
use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer, IMFActivate, IMFMediaBuffer, IMFSample, IMFTransform, MF_TRANSFORM_ASYNC,
    MFMediaType_Video, MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG, MFT_ENUM_FLAG_SORTANDFILTER,
    MFT_ENUM_FLAG_SYNCMFT, MFT_REGISTER_TYPE_INFO, MFTEnumEx, MFVideoFormat_NV12,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::core::{GUID, Interface};

/// Open a software (synchronous) H.264 decoder MFT — no D3D device required.
pub(super) fn open_sw_decoder(input_subtype: &GUID) -> Result<IMFTransform, DecodeError> {
    let transform = activate_sw_decoder(input_subtype)?;
    ensure_synchronous(&transform)?;
    Ok(transform)
}

fn activate_sw_decoder(input_subtype: &GUID) -> Result<IMFTransform, DecodeError> {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: *input_subtype,
    };
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let flags = MFT_ENUM_FLAG(MFT_ENUM_FLAG_SYNCMFT.0 | MFT_ENUM_FLAG_SORTANDFILTER.0);
    let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    // SAFETY: MFTEnumEx writes an activate-object array + count as out-params.
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_DECODER,
            flags,
            Some(std::ptr::from_ref(&input)),
            Some(std::ptr::from_ref(&output)),
            &raw mut activates,
            &raw mut count,
        )
    }
    .map_err(|_| DecodeError::Unsupported)?;

    if activates.is_null() || count == 0 {
        return Err(DecodeError::Unsupported);
    }

    let mut chosen: Option<IMFTransform> = None;
    for i in 0..count as usize {
        // SAFETY: `activates` holds `count` valid `Option<IMFActivate>` slots from MFTEnumEx.
        let activate = unsafe { (*activates.add(i)).take() };
        let Some(activate) = activate else {
            continue;
        };
        if let Ok(t) = unsafe { activate.ActivateObject::<IMFTransform>() } {
            chosen = Some(t);
            break;
        }
    }
    // SAFETY: `activates` was allocated by MFTEnumEx (CoTaskMemAlloc); we own and free it.
    unsafe {
        CoTaskMemFree(Some(activates.cast_const().cast()));
    }
    chosen.ok_or(DecodeError::Unsupported)
}

/// The CPU path drives `ProcessInput`/`ProcessOutput` synchronously; an async MFT would need
/// the same `IMFMediaEventGenerator` pump as the DX11 path, which this module does not
/// implement (software H.264 decoder MFTs are documented as synchronous in practice).
fn ensure_synchronous(transform: &IMFTransform) -> Result<(), DecodeError> {
    let attrs = unsafe { transform.GetAttributes() }.map_err(|_| DecodeError::Backend)?;
    let is_async = unsafe { attrs.GetUINT32(&MF_TRANSFORM_ASYNC) }.unwrap_or(0) != 0;
    if is_async {
        return Err(DecodeError::Unsupported);
    }
    Ok(())
}

/// Copy NV12 planes out of a decoder CPU output sample into tightly packed bytes
/// (`width * height` luma followed by `width * height / 2` interleaved chroma).
pub(super) fn nv12_bytes_from_output_sample(
    sample: &IMFSample,
    width: u32,
    height: u32,
) -> Result<Bytes, DecodeError> {
    // SAFETY: decoder output sample carries exactly one system-memory buffer.
    let buffer = unsafe { sample.GetBufferByIndex(0) }.map_err(|_| DecodeError::Backend)?;
    if let Ok(buf2d) = buffer.cast::<IMF2DBuffer>() {
        return copy_2d_nv12(&buf2d, width, height);
    }
    copy_contiguous_nv12(&buffer)
}

fn copy_2d_nv12(buf2d: &IMF2DBuffer, width: u32, height: u32) -> Result<Bytes, DecodeError> {
    let mut scanline0: *mut u8 = std::ptr::null_mut();
    let mut pitch = 0i32;
    // SAFETY: out-params written on success; buffer stays locked until Unlock2D below.
    unsafe {
        buf2d
            .Lock2D(&raw mut scanline0, &raw mut pitch)
            .map_err(|_| DecodeError::Backend)?;
    }
    if scanline0.is_null() {
        // SAFETY: matching Unlock2D for the successful Lock2D above.
        let _ = unsafe { buf2d.Unlock2D() };
        return Err(DecodeError::Backend);
    }
    let width_usize = width as usize;
    let height_usize = height as usize;
    let pitch_usize = pitch.unsigned_abs() as usize;
    let mut out = vec![0u8; width_usize * height_usize + width_usize * (height_usize / 2)];
    // SAFETY: `scanline0` is locked for at least `height` luma rows plus `height / 2`
    // interleaved-chroma rows at `pitch_usize` stride, matching the NV12 layout the
    // decoder wrote; each copied row stays within both the source and `out`.
    unsafe {
        for row in 0..height_usize {
            let src = scanline0.add(row * pitch_usize);
            let dst = out.as_mut_ptr().add(row * width_usize);
            std::ptr::copy_nonoverlapping(src, dst, width_usize);
        }
        let uv_src_base = scanline0.add(height_usize * pitch_usize);
        let uv_dst_base = out.as_mut_ptr().add(width_usize * height_usize);
        for row in 0..height_usize / 2 {
            let src = uv_src_base.add(row * pitch_usize);
            let dst = uv_dst_base.add(row * width_usize);
            std::ptr::copy_nonoverlapping(src, dst, width_usize);
        }
    }
    // SAFETY: matching Unlock2D for the successful Lock2D above.
    unsafe {
        buf2d.Unlock2D().map_err(|_| DecodeError::Backend)?;
    }
    Ok(Bytes::from(out))
}

fn copy_contiguous_nv12(buffer: &IMFMediaBuffer) -> Result<Bytes, DecodeError> {
    let mut ptr: *mut u8 = std::ptr::null_mut();
    let mut cur_len = 0u32;
    // SAFETY: out-params written on success; buffer stays locked until Unlock below.
    unsafe {
        buffer
            .Lock(&raw mut ptr, None, Some(std::ptr::from_mut(&mut cur_len)))
            .map_err(|_| DecodeError::Backend)?;
    }
    if ptr.is_null() {
        // SAFETY: matching Unlock for the successful Lock above.
        let _ = unsafe { buffer.Unlock() };
        return Err(DecodeError::Backend);
    }
    // SAFETY: `ptr` is valid for `cur_len` bytes for the duration of the lock; copied out
    // as an owned `Vec` before Unlock releases it.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, cur_len as usize) }.to_vec();
    // SAFETY: matching Unlock for the successful Lock above.
    unsafe {
        buffer.Unlock().map_err(|_| DecodeError::Backend)?;
    }
    Ok(Bytes::from(bytes))
}

#[cfg(test)]
#[path = "cpu_tests.rs"]
mod tests;
