//! Shared sync-MFT plumbing for the WMF audio decode sessions (`opus`, `aac`).
//!
//! Both sessions drive a *sync* inbox decoder MFT the same way: allocate the output
//! sample ourselves (a sync MFT does not provide one), `ProcessOutput`, treat
//! `MF_E_TRANSFORM_NEED_MORE_INPUT` as "not an error, just no frame yet", and copy the
//! locked buffer out. Only media-type negotiation differs between the two codecs, so
//! that stays in each session's own module.
//!
//! This is deliberately **not** `wmf::shared`, which is `#[cfg(feature = "video")]` and
//! carries video-specific helpers — an audio-only build must not need the video feature.

#![allow(unsafe_code)]

use crate::DecodeError;
use mediaway_common::{Bytes, Packet};
use windows::Win32::Media::MediaFoundation::{
    IMFMediaBuffer, IMFSample, IMFTransform, MF_E_TRANSFORM_NEED_MORE_INPUT, MFCreateMemoryBuffer,
    MFCreateSample, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_END_STREAMING,
    MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
};

use super::runtime::to_hns;

/// One `ProcessOutput` result.
pub(super) enum Drain {
    /// A decoded PCM buffer came out.
    Frame(OutputPayload),
    /// The MFT wants more input before it will produce anything.
    NeedMore,
}

/// Raw decoded PCM plus the sample time the MFT stamped on it.
pub(super) struct OutputPayload {
    pub(super) data: Bytes,
    pub(super) pts_hns: i64,
}

/// Put the MFT into streaming state.
pub(super) fn begin_streaming(transform: &IMFTransform) -> Result<(), DecodeError> {
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

/// The MFT's requested output buffer size (at least 1 byte).
pub(super) fn output_buffer_size(transform: &IMFTransform) -> Result<u32, DecodeError> {
    let out_info = unsafe { transform.GetOutputStreamInfo(0) }.map_err(|_| DecodeError::Backend)?;
    Ok(out_info.cbSize.max(1))
}

/// Pull at most one decoded buffer out of `transform`.
pub(super) fn process_one_output(
    transform: &IMFTransform,
    output_buf_size: u32,
) -> Result<Drain, DecodeError> {
    let mut status = 0u32;
    // SAFETY: allocate an output sample + memory buffer for this sync MFT (it does not
    // provide its own output samples).
    let out_sample: IMFSample = unsafe { MFCreateSample() }.map_err(|_| DecodeError::Backend)?;
    let out_buffer =
        unsafe { MFCreateMemoryBuffer(output_buf_size) }.map_err(|_| DecodeError::Backend)?;
    unsafe { out_sample.AddBuffer(&out_buffer) }.map_err(|_| DecodeError::Backend)?;
    let mut buffers = [MFT_OUTPUT_DATA_BUFFER {
        dwStreamID: 0,
        pSample: std::mem::ManuallyDrop::new(Some(out_sample)),
        dwStatus: 0,
        pEvents: std::mem::ManuallyDrop::new(None),
    }];

    // SAFETY: ProcessOutput; HRESULT inspected below.
    let hr = unsafe { transform.ProcessOutput(0, &mut buffers, &raw mut status) };
    let sample = unsafe { std::mem::ManuallyDrop::take(&mut buffers[0].pSample) };
    let _ = unsafe { std::mem::ManuallyDrop::take(&mut buffers[0].pEvents) };

    if let Err(e) = hr {
        if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT {
            return Ok(Drain::NeedMore);
        }
        return Err(DecodeError::Backend);
    }
    let Some(sample) = sample else {
        return Ok(Drain::NeedMore);
    };
    Ok(Drain::Frame(payload_from_sample(&sample)?))
}

/// Copy a decoded sample's contiguous buffer into owned [`Bytes`].
pub(super) fn payload_from_sample(sample: &IMFSample) -> Result<OutputPayload, DecodeError> {
    let buffer = unsafe { sample.ConvertToContiguousBuffer() }.map_err(|_| DecodeError::Backend)?;
    let mut ptr = std::ptr::null_mut();
    let mut cur_len = 0u32;
    unsafe {
        buffer
            .Lock(&raw mut ptr, None, Some(std::ptr::from_mut(&mut cur_len)))
            .map_err(|_| DecodeError::Backend)?;
    }
    if ptr.is_null() {
        unsafe {
            let _: windows::core::Result<()> = buffer.Unlock();
        }
        return Err(DecodeError::Backend);
    }
    let mut data = vec![0u8; cur_len as usize];
    unsafe {
        std::ptr::copy_nonoverlapping(ptr, data.as_mut_ptr(), cur_len as usize);
        buffer.Unlock().map_err(|_| DecodeError::Backend)?;
    }
    let pts_hns = unsafe { sample.GetSampleTime() }.unwrap_or(0);
    Ok(OutputPayload {
        data: Bytes::from(data),
        pts_hns,
    })
}

/// Wrap `packet`'s compressed payload in an `IMFSample` stamped with its presentation time.
///
/// An empty payload is rejected: neither decoder MFT can infer a frame layout from zero
/// bytes (Opus still needs its TOC byte even for a DTX frame; raw AAC needs a whole
/// `raw_data_block()`).
pub(super) fn packet_to_sample(
    packet: &Packet,
    time_base_num: u64,
    time_base_den: u32,
) -> Result<IMFSample, DecodeError> {
    if packet.payload.is_empty() {
        return Err(DecodeError::InvalidInput);
    }
    let len = u32::try_from(packet.payload.len()).map_err(|_| DecodeError::InvalidInput)?;
    let sample: IMFSample = unsafe { MFCreateSample() }.map_err(|_| DecodeError::Backend)?;
    let buffer: IMFMediaBuffer =
        unsafe { MFCreateMemoryBuffer(len) }.map_err(|_| DecodeError::Backend)?;
    unsafe {
        let mut ptr = std::ptr::null_mut();
        let mut max_len = 0u32;
        buffer
            .Lock(&raw mut ptr, Some(std::ptr::from_mut(&mut max_len)), None)
            .map_err(|_| DecodeError::Backend)?;
        if ptr.is_null() || max_len < len {
            let _: windows::core::Result<()> = buffer.Unlock();
            return Err(DecodeError::Backend);
        }
        std::ptr::copy_nonoverlapping(packet.payload.as_ref().as_ptr(), ptr, packet.payload.len());
        buffer
            .SetCurrentLength(len)
            .map_err(|_| DecodeError::Backend)?;
        buffer.Unlock().map_err(|_| DecodeError::Backend)?;
    }
    unsafe { sample.AddBuffer(&buffer) }.map_err(|_| DecodeError::Backend)?;

    let hns = to_hns(packet.pts, time_base_num, time_base_den);
    unsafe {
        sample
            .SetSampleTime(hns)
            .map_err(|_| DecodeError::Backend)?;
    }
    Ok(sample)
}

/// Best-effort "streaming finished" notification; failures are not actionable.
pub(super) fn notify_end_streaming(transform: &IMFTransform) {
    unsafe {
        let _: windows::core::Result<()> =
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
    }
}
