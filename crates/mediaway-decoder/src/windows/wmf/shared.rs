//! Shared MF H.264 decode helpers (DX11 Zero-Copy path).

#![allow(unsafe_code)]

use crate::DecodeError;
use mediaway_common::{Bytes, Packet};
use windows::Win32::Media::MediaFoundation::{
    IMFMediaBuffer, IMFSample, IMFTransform, MF_E_TRANSFORM_NEED_MORE_INPUT,
    MF_E_TRANSFORM_STREAM_CHANGE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE,
    MF_MT_MPEG_SEQUENCE_HEADER, MF_MT_SUBTYPE, MFCreateMediaType, MFCreateMemoryBuffer,
    MFCreateSample, MFMediaType_Video, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
    MFT_MESSAGE_NOTIFY_END_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
    MFVideoFormat_NV12, MFVideoInterlace_Progressive,
};
use windows::core::GUID;

use super::runtime::{pack_u32_pair, unpack_u32_pair};

pub(super) enum Drain {
    Sample(IMFSample),
    NeedMore,
    StreamChange,
}

pub(super) fn configure_decode_types(
    transform: &IMFTransform,
    width: u32,
    height: u32,
    extra_data: &Bytes,
    input_subtype: &GUID,
) -> Result<(), DecodeError> {
    // SAFETY: owned media types; plain attribute setters.
    let in_type = unsafe { MFCreateMediaType() }.map_err(|_| DecodeError::Backend)?;
    unsafe {
        in_type
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(|_| DecodeError::Backend)?;
        in_type
            .SetGUID(&MF_MT_SUBTYPE, input_subtype)
            .map_err(|_| DecodeError::Backend)?;
        if width > 0 && height > 0 {
            in_type
                .SetUINT64(&MF_MT_FRAME_SIZE, pack_u32_pair(width, height))
                .map_err(|_| DecodeError::Backend)?;
        }
        in_type
            .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
            .map_err(|_| DecodeError::Backend)?;
        if !extra_data.is_empty() {
            in_type
                .SetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, extra_data.as_ref())
                .map_err(|_| DecodeError::Backend)?;
        }
        transform
            .SetInputType(0, &in_type, 0)
            .map_err(|_| DecodeError::Backend)?;
    }

    let out_type = unsafe { MFCreateMediaType() }.map_err(|_| DecodeError::Backend)?;
    unsafe {
        out_type
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(|_| DecodeError::Backend)?;
        out_type
            .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)
            .map_err(|_| DecodeError::Backend)?;
        if width > 0 && height > 0 {
            out_type
                .SetUINT64(&MF_MT_FRAME_SIZE, pack_u32_pair(width, height))
                .map_err(|_| DecodeError::Backend)?;
        }
        out_type
            .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
            .map_err(|_| DecodeError::Backend)?;
        transform
            .SetOutputType(0, &out_type, 0)
            .map_err(|_| DecodeError::Backend)?;
    }
    Ok(())
}

pub(super) fn begin_streaming(transform: &IMFTransform) -> Result<(), DecodeError> {
    // SAFETY: stream control messages; no user pointers.
    unsafe {
        transform
            .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
            .map_err(|_| DecodeError::Backend)?;
        transform
            .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
            .map_err(|_| DecodeError::Backend)?;
    }
    Ok(())
}

pub(super) fn process_one_output(
    transform: &IMFTransform,
    output_provides_samples: bool,
    output_buf_size: u32,
) -> Result<Drain, DecodeError> {
    let mut status = 0u32;
    let mut buffers = if output_provides_samples {
        [MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: std::mem::ManuallyDrop::new(None),
            dwStatus: 0,
            pEvents: std::mem::ManuallyDrop::new(None),
        }]
    } else {
        // SAFETY: allocate output sample + a media buffer of the MFT-reported size for
        // sync MFTs that do not provide their own samples (e.g. the software H.264 decoder).
        let out_sample: IMFSample =
            unsafe { MFCreateSample() }.map_err(|_| DecodeError::Backend)?;
        let out_buffer =
            unsafe { MFCreateMemoryBuffer(output_buf_size) }.map_err(|_| DecodeError::Backend)?;
        unsafe { out_sample.AddBuffer(&out_buffer) }.map_err(|_| DecodeError::Backend)?;
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
        return Err(DecodeError::Backend);
    }
    let Some(sample) = sample else {
        return Ok(Drain::NeedMore);
    };
    Ok(Drain::Sample(sample))
}

pub(super) fn packet_to_sample(
    packet: &Packet,
    time_base_num: u64,
    time_base_den: u32,
    nal_length_size: Option<u8>,
) -> Result<IMFSample, DecodeError> {
    if packet.payload.is_empty() {
        return Err(DecodeError::InvalidInput);
    }
    // AVCC-framed packets (demuxed MP4 samples) need converting to Annex-B before the
    // MFT can find NAL start codes; Annex-B packets (e.g. straight from an encoder)
    // pass through unchanged.
    let annex_b_payload = nal_length_size.map_or_else(
        || packet.payload.clone(), // clone: Bytes ref-count bump, not a payload copy
        |n| iso_bmff::bitstream::avc::avcc_payload_to_annex_b(&packet.payload, n),
    );
    if annex_b_payload.is_empty() {
        return Err(DecodeError::InvalidInput);
    }
    let len = u32::try_from(annex_b_payload.len()).map_err(|_| DecodeError::InvalidInput)?;
    let sample: IMFSample = unsafe { MFCreateSample() }.map_err(|_| DecodeError::Backend)?;
    let buffer: IMFMediaBuffer =
        unsafe { MFCreateMemoryBuffer(len) }.map_err(|_| DecodeError::Backend)?;
    unsafe {
        let mut ptr = std::ptr::null_mut();
        let mut max_len = 0u32;
        let mut cur_len = 0u32;
        buffer
            .Lock(
                &raw mut ptr,
                Some(std::ptr::from_mut(&mut max_len)),
                Some(std::ptr::from_mut(&mut cur_len)),
            )
            .map_err(|_| DecodeError::Backend)?;
        if ptr.is_null() || max_len < len {
            let _: windows::core::Result<()> = buffer.Unlock();
            return Err(DecodeError::Backend);
        }
        std::ptr::copy_nonoverlapping(annex_b_payload.as_ptr(), ptr, annex_b_payload.len());
        buffer
            .SetCurrentLength(len)
            .map_err(|_| DecodeError::Backend)?;
        buffer.Unlock().map_err(|_| DecodeError::Backend)?;
    }
    unsafe { sample.AddBuffer(&buffer) }.map_err(|_| DecodeError::Backend)?;

    let hns = super::runtime::to_hns(packet.pts, time_base_num, time_base_den);
    let dur = super::runtime::to_hns(
        i64::try_from(packet.duration).unwrap_or(0),
        time_base_num,
        time_base_den,
    )
    .max(1);
    unsafe {
        sample
            .SetSampleTime(hns)
            .map_err(|_| DecodeError::Backend)?;
        sample
            .SetSampleDuration(dur)
            .map_err(|_| DecodeError::Backend)?;
    }
    Ok(sample)
}

/// Query the MFT-reported output buffer size (for MFTs that do not provide their own samples).
pub(super) fn output_buffer_size(transform: &IMFTransform) -> Result<u32, DecodeError> {
    let out_info = unsafe { transform.GetOutputStreamInfo(0) }.map_err(|_| DecodeError::Backend)?;
    Ok(out_info.cbSize.max(1))
}

/// Read `MF_MT_FRAME_SIZE` from the current output type when the MFT signals stream change.
pub(super) fn read_output_dimensions(transform: &IMFTransform) -> Result<(u32, u32), DecodeError> {
    let mt = unsafe { transform.GetOutputCurrentType(0) }.map_err(|_| DecodeError::Backend)?;
    let packed = unsafe { mt.GetUINT64(&MF_MT_FRAME_SIZE) }.map_err(|_| DecodeError::Backend)?;
    let (width, height) = unpack_u32_pair(packed);
    if width == 0 || height == 0 {
        return Err(DecodeError::Backend);
    }
    Ok((width, height))
}

/// Post-drain shutdown notification; ignore errors if the MFT already ended.
pub(super) fn notify_end_streaming(transform: &IMFTransform) {
    unsafe {
        let _: windows::core::Result<()> =
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
    }
}
