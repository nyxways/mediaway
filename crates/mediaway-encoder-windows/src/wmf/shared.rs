//! Shared MF video encode helpers (CPU + DX11 paths).

#![allow(unsafe_code)]

use mediaway_common::{Bytes, Packet, PixelFormat, StreamInfo};
use mediaway_encoder::EncodeError;
use windows::Win32::Media::MediaFoundation::{
    IMFMediaBuffer, IMFSample, IMFTransform, MF_E_TRANSFORM_NEED_MORE_INPUT,
    MF_E_TRANSFORM_STREAM_CHANGE, MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
    MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MFCreateMediaType, MFCreateMemoryBuffer,
    MFCreateSample, MFMediaType_Video, MFSampleExtension_CleanPoint,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_END_STREAMING,
    MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER, MFVideoFormat_ARGB32,
    MFVideoFormat_NV12, MFVideoInterlace_Progressive,
};
use windows::core::GUID;

use super::runtime::pack_u32_pair;

pub(super) enum Drain {
    Packet(Packet),
    NeedMore,
    StreamChange,
}

pub(super) fn bitrate_and_fps(
    config_bitrate: u32,
    time_base_num: u64,
    time_base_den: u32,
) -> (u32, u32, u32) {
    let fps_num = time_base_den;
    let fps_den = u32::try_from(time_base_num.max(1)).unwrap_or(1);
    let bitrate = if config_bitrate == 0 {
        2_000_000
    } else {
        config_bitrate
    };
    (bitrate, fps_num, fps_den)
}

pub(super) fn nv12_size(width: u32, height: u32) -> Result<usize, EncodeError> {
    let w = width as usize;
    let h = height as usize;
    w.checked_mul(h)
        .and_then(|y| y.checked_add(y / 2))
        .ok_or(EncodeError::InvalidInput)
}

pub(super) fn configure_types(
    transform: &IMFTransform,
    width: u32,
    height: u32,
    fps_num: u32,
    fps_den: u32,
    bitrate: u32,
    output_subtype: &GUID,
    input_pixel_format: PixelFormat,
) -> Result<(), EncodeError> {
    // SAFETY: owned media types; plain attribute setters.
    let out_type = unsafe { MFCreateMediaType() }.map_err(|_| EncodeError::Backend)?;
    unsafe {
        out_type
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(|_| EncodeError::Backend)?;
        out_type
            .SetGUID(&MF_MT_SUBTYPE, output_subtype)
            .map_err(|_| EncodeError::Backend)?;
        out_type
            .SetUINT64(&MF_MT_FRAME_SIZE, pack_u32_pair(width, height))
            .map_err(|_| EncodeError::Backend)?;
        out_type
            .SetUINT64(&MF_MT_FRAME_RATE, pack_u32_pair(fps_num, fps_den))
            .map_err(|_| EncodeError::Backend)?;
        out_type
            .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
            .map_err(|_| EncodeError::Backend)?;
        out_type
            .SetUINT32(&MF_MT_AVG_BITRATE, bitrate)
            .map_err(|_| EncodeError::Backend)?;
        transform
            .SetOutputType(0, &out_type, 0)
            .map_err(|_| EncodeError::Backend)?;
    }

    // Prefer ARGB32 for BGRA desktop/WGC surfaces (live-recorder pattern) — Zero-Copy, no NV12 convert.
    // Fall back to NV12 when the MFT rejects ARGB32.
    let attempts: &[GUID] = match input_pixel_format {
        PixelFormat::Bgra8 => &[MFVideoFormat_ARGB32, MFVideoFormat_NV12],
        PixelFormat::Nv12 => &[MFVideoFormat_NV12],
        _ => return Err(EncodeError::Unsupported),
    };
    let mut last = EncodeError::Unsupported;
    for subtype in attempts {
        let in_type = unsafe { MFCreateMediaType() }.map_err(|_| EncodeError::Backend)?;
        let ok = unsafe {
            in_type
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .is_ok()
                && in_type.SetGUID(&MF_MT_SUBTYPE, subtype).is_ok()
                && in_type
                    .SetUINT64(&MF_MT_FRAME_SIZE, pack_u32_pair(width, height))
                    .is_ok()
                && in_type
                    .SetUINT64(&MF_MT_FRAME_RATE, pack_u32_pair(fps_num, fps_den))
                    .is_ok()
                && in_type
                    .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
                    .is_ok()
                && transform.SetInputType(0, &in_type, 0).is_ok()
        };
        if ok {
            return Ok(());
        }
        last = EncodeError::Backend;
    }
    Err(last)
}

pub(super) fn begin_streaming(transform: &IMFTransform) -> Result<(), EncodeError> {
    // SAFETY: stream control messages; no user pointers.
    unsafe {
        transform
            .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
            .map_err(|_| EncodeError::Backend)?;
        transform
            .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
            .map_err(|_| EncodeError::Backend)?;
    }
    Ok(())
}

pub(super) fn output_buffer_hint(transform: &IMFTransform) -> Result<u32, EncodeError> {
    // SAFETY: POD size hint.
    let out_info = unsafe { transform.GetOutputStreamInfo(0) }.map_err(|_| EncodeError::Backend)?;
    Ok(out_info.cbSize.max(1))
}

pub(super) fn process_one_output(
    transform: &IMFTransform,
    output_buf_size: u32,
    output_provides_samples: bool,
    info: &StreamInfo,
) -> Result<Drain, EncodeError> {
    let mut status = 0u32;
    let mut buffers = if output_provides_samples {
        [MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: std::mem::ManuallyDrop::new(None),
            dwStatus: 0,
            pEvents: std::mem::ManuallyDrop::new(None),
        }]
    } else {
        // SAFETY: allocate output sample for sync MFTs.
        let out_sample: IMFSample =
            unsafe { MFCreateSample() }.map_err(|_| EncodeError::Backend)?;
        let out_buf: IMFMediaBuffer =
            unsafe { MFCreateMemoryBuffer(output_buf_size) }.map_err(|_| EncodeError::Backend)?;
        unsafe { out_sample.AddBuffer(&out_buf) }.map_err(|_| EncodeError::Backend)?;
        [MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: std::mem::ManuallyDrop::new(Some(out_sample)),
            dwStatus: 0,
            pEvents: std::mem::ManuallyDrop::new(None),
        }]
    };

    // SAFETY: ProcessOutput; HRESULT inspected below.
    let hr = unsafe { transform.ProcessOutput(0, &mut buffers, &raw mut status) };
    let sample = unsafe { std::mem::ManuallyDrop::take(&mut buffers[0].pSample) };
    let _ = unsafe { std::mem::ManuallyDrop::take(&mut buffers[0].pEvents) };

    if let Err(e) = hr {
        if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT {
            return Ok(Drain::NeedMore);
        }
        if e.code() == MF_E_TRANSFORM_STREAM_CHANGE {
            return Ok(Drain::StreamChange);
        }
        return Err(EncodeError::Backend);
    }
    let Some(sample) = sample else {
        return Ok(Drain::NeedMore);
    };
    Ok(Drain::Packet(sample_to_packet(&sample, info)?))
}

pub(super) fn sample_to_packet(
    sample: &IMFSample,
    info: &StreamInfo,
) -> Result<Packet, EncodeError> {
    // SAFETY: contiguous buffer lock for read.
    let buffer = unsafe { sample.ConvertToContiguousBuffer() }.map_err(|_| EncodeError::Backend)?;
    let mut ptr = std::ptr::null_mut();
    let mut max_len = 0u32;
    let mut cur_len = 0u32;
    unsafe {
        buffer
            .Lock(
                &raw mut ptr,
                Some(std::ptr::from_mut(&mut max_len)),
                Some(std::ptr::from_mut(&mut cur_len)),
            )
            .map_err(|_| EncodeError::Backend)?;
    }
    if ptr.is_null() || cur_len == 0 {
        unsafe {
            let _: windows::core::Result<()> = buffer.Unlock();
        }
        return Err(EncodeError::Backend);
    }
    let mut payload = vec![0u8; cur_len as usize];
    unsafe {
        std::ptr::copy_nonoverlapping(ptr, payload.as_mut_ptr(), cur_len as usize);
        buffer.Unlock().map_err(|_| EncodeError::Backend)?;
    }

    let time_base = info.time_base();
    let pts_hns = unsafe { sample.GetSampleTime() }.unwrap_or(0);
    let dur_hns = unsafe { sample.GetSampleDuration() }.unwrap_or(0);
    let pts = from_hns(pts_hns, time_base.num, time_base.den);
    let duration =
        u64::try_from(from_hns(dur_hns, time_base.num, time_base.den).max(0)).unwrap_or(0);
    let is_keyframe = unsafe { sample.GetUINT32(&MFSampleExtension_CleanPoint) }.unwrap_or(0) != 0;

    Ok(Packet {
        stream_id: info.id(),
        pts,
        dts: pts,
        duration,
        is_keyframe,
        is_discard: false,
        payload: Bytes::from(payload),
    })
}

fn from_hns(hns: i64, time_base_num: u64, time_base_den: u32) -> i64 {
    if time_base_num == 0 || time_base_den == 0 {
        return 0;
    }
    let num = i128::from(hns) * i128::from(time_base_den);
    let den = i128::from(time_base_num) * 10_000_000;
    i64::try_from(num / den).unwrap_or(0)
}

/// Post-drain shutdown notification; ignore errors if the MFT already ended.
pub(super) fn notify_end_streaming(transform: &IMFTransform) {
    unsafe {
        let _: windows::core::Result<()> =
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
    }
}
