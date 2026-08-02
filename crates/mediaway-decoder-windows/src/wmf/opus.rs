//! Opus decode session: inbox WMF Opus decoder MFT (`CMSOpusDecMFT`, Float32 PCM out).
//!
//! Research finding (this session, real `MFTEnumEx` + `CoCreateInstance` verification on
//! an actual Windows 11 box): Windows ships an inbox Opus **decoder** MFT
//! (`CLSID_MSOpusDecoder` / `CMSOpusDecMFT`,
//! `{63E17C10-2D43-4C42-8FE3-8D8B63E46A6A}`) but **no** inbox Opus **encoder** MFT —
//! `MFTEnumEx(MFT_CATEGORY_AUDIO_ENCODER, ..., MFAudioFormat_Opus)` returns zero results,
//! and none of the 9 registered audio encoder MFTs on that machine mention Opus. The
//! `windows` crate's Media Foundation bindings only expose a decoder CLSID constant
//! (`CLSID_MSOpusDecoder` / `CMSOpusDecMFT`); no encoder CLSID exists. There is therefore
//! no encode-side counterpart to this module.
//!
//! The decoder MFT only ever offers one output type: `MFAudioFormat_Float` (32-bit IEEE
//! float) at the input sample rate/channel count — a hand-built 16-bit PCM output type is
//! rejected (`MF_E_INVALIDMEDIATYPE`), so this session negotiates the output type by
//! querying [`IMFTransform::GetOutputAvailableType`] after the input type is set, rather
//! than constructing one. Verified end-to-end with a real (RFC 6716 section 3.1) minimal
//! 1-byte Opus packet (TOC-only, SILK NB 10 ms, packet loss/DTX frame) — `ProcessInput` +
//! `ProcessOutput` produced a real 3840-byte (960 float samples, 2ch x 480/ch = 10 ms
//! @ 48 kHz) PCM buffer.
//!
//! Not yet wired into any public backend entry point: `mediaway-decoder` has no
//! `AudioDecoder` trait today (only `VideoDecoder`), so there is no facade shape for this
//! module to implement against yet. Designing that trait is a facade-level API decision
//! (ADR-worthy) out of scope here; this module stays a self-contained, real, and tested
//! MFT session so a later integration pass can wire it in without redesigning the MFT
//! plumbing. See `docs/roadmap.md`.

#![allow(unsafe_code)]
#![allow(
    dead_code,
    reason = "not yet wired into a public entry point — no `AudioDecoder` trait exists yet \
              in `mediaway-decoder` to implement against; see module docs / roadmap"
)]

use std::collections::VecDeque;

use mediaway_common::{AudioFrame, Bytes, CodecKind, Packet, Rational, SampleFormat, StreamInfo};
use mediaway_decoder::DecodeError;
use windows::Win32::Media::MediaFoundation::{
    CMSOpusDecMFT, IMFMediaBuffer, IMFSample, IMFTransform, MF_E_TRANSFORM_NEED_MORE_INPUT,
    MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
    MFAudioFormat_Opus, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample, MFMediaType_Audio,
    MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
    MFT_MESSAGE_NOTIFY_END_OF_STREAM, MFT_MESSAGE_NOTIFY_END_STREAMING,
    MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};

use super::runtime::{from_hns, to_hns};

/// Config for [`WmfOpusDecoder::open`]. Deliberately not `mediaway_decoder::*` — no
/// `AudioDecoder`/`AudioDecoderConfig` shape exists in the facade yet (see module docs).
pub(crate) struct OpusDecoderConfig {
    pub(crate) sample_rate: u32,
    pub(crate) channels: u16,
    /// Stream timebase; audio sessions elsewhere in this workspace use `1 / sample_rate`
    /// so `Packet`/`AudioFrame` `pts`/`duration` are plain sample counts.
    pub(crate) time_base: Rational,
}

/// Opus decode session (WMF `CMSOpusDecMFT`, Float32 PCM output; see module docs).
pub(crate) struct WmfOpusDecoder {
    transform: IMFTransform,
    info: StreamInfo,
    time_base_num: u64,
    time_base_den: u32,
    channels: u16,
    output_buf_size: u32,
    pending: VecDeque<AudioFrame>,
    flushed: bool,
}

impl WmfOpusDecoder {
    /// Open a WMF Opus decoder for `config`.
    pub(crate) fn open(config: &OpusDecoderConfig) -> Result<Self, DecodeError> {
        validate(config)?;
        super::runtime::ensure_mf()?;

        // SAFETY: inbox sync Opus decoder MFT.
        let transform: IMFTransform =
            unsafe { CoCreateInstance(&CMSOpusDecMFT, None, CLSCTX_INPROC_SERVER) }
                .map_err(|_| DecodeError::Backend)?;

        configure_types(&transform, config.sample_rate, config.channels)?;
        begin_streaming(&transform)?;
        let output_buf_size = output_buffer_size(&transform)?;

        Ok(Self {
            transform,
            info: stream_info_from(config),
            time_base_num: config.time_base.num,
            time_base_den: config.time_base.den,
            channels: config.channels,
            output_buf_size,
            pending: VecDeque::new(),
            flushed: false,
        })
    }

    pub(crate) const fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    pub(crate) fn push_packet(&mut self, packet: &Packet) -> Result<(), DecodeError> {
        if self.flushed {
            return Err(DecodeError::Closed);
        }
        if packet.is_discard {
            return Ok(());
        }
        let sample = packet_to_sample(packet, self.time_base_num, self.time_base_den)?;
        unsafe { self.transform.ProcessInput(0, &sample, 0) }.map_err(|_| DecodeError::Backend)?;
        self.drain_output()
    }

    pub(crate) fn poll_frame(&mut self) -> Result<Option<AudioFrame>, DecodeError> {
        if self.pending.is_empty() {
            self.drain_output()?;
        }
        Ok(self.pending.pop_front())
    }

    pub(crate) fn flush(&mut self) -> Result<(), DecodeError> {
        if self.flushed {
            return Ok(());
        }
        self.flushed = true;
        unsafe {
            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)
                .map_err(|_| DecodeError::Backend)?;
            self.transform
                .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)
                .map_err(|_| DecodeError::Backend)?;
        }
        self.drain_output()?;
        notify_end_streaming(&self.transform);
        Ok(())
    }

    fn drain_output(&mut self) -> Result<(), DecodeError> {
        while let Drain::Frame(payload) = process_one_output(&self.transform, self.output_buf_size)?
        {
            self.pending.push_back(self.frame_from_payload(payload));
        }
        Ok(())
    }

    fn frame_from_payload(&self, payload: OutputPayload) -> AudioFrame {
        let channels = usize::from(self.channels).max(1);
        let samples_per_channel = payload.data.len() / 4 / channels;
        let pts = from_hns(payload.pts_hns, self.time_base_num, self.time_base_den);
        AudioFrame {
            pts,
            duration: u64::try_from(samples_per_channel).unwrap_or(0),
            sample_rate: self.info.sample_rate().unwrap_or(0),
            channels: self.channels,
            format: SampleFormat::F32,
            data: payload.data,
        }
    }
}

enum Drain {
    Frame(OutputPayload),
    NeedMore,
}

struct OutputPayload {
    data: Bytes,
    pts_hns: i64,
}

fn configure_types(
    transform: &IMFTransform,
    sample_rate: u32,
    channels: u16,
) -> Result<(), DecodeError> {
    let in_type = unsafe { MFCreateMediaType() }.map_err(|_| DecodeError::Backend)?;
    unsafe {
        in_type
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
            .map_err(|_| DecodeError::Backend)?;
        in_type
            .SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_Opus)
            .map_err(|_| DecodeError::Backend)?;
        in_type
            .SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, u32::from(channels))
            .map_err(|_| DecodeError::Backend)?;
        in_type
            .SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, sample_rate)
            .map_err(|_| DecodeError::Backend)?;
        transform
            .SetInputType(0, &in_type, 0)
            .map_err(|_| DecodeError::Backend)?;
    }

    // The decoder only ever proposes one output type (Float32 PCM at the negotiated
    // rate/channels) — take its own proposal rather than hand-building one (a hand-built
    // 16-bit PCM output type is rejected; verified on real hardware, see module docs).
    let out_type =
        unsafe { transform.GetOutputAvailableType(0, 0) }.map_err(|_| DecodeError::Backend)?;
    unsafe {
        transform
            .SetOutputType(0, &out_type, 0)
            .map_err(|_| DecodeError::Backend)?;
    }
    Ok(())
}

fn begin_streaming(transform: &IMFTransform) -> Result<(), DecodeError> {
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

fn output_buffer_size(transform: &IMFTransform) -> Result<u32, DecodeError> {
    let out_info = unsafe { transform.GetOutputStreamInfo(0) }.map_err(|_| DecodeError::Backend)?;
    Ok(out_info.cbSize.max(1))
}

fn process_one_output(
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

fn payload_from_sample(sample: &IMFSample) -> Result<OutputPayload, DecodeError> {
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

fn packet_to_sample(
    packet: &Packet,
    time_base_num: u64,
    time_base_den: u32,
) -> Result<IMFSample, DecodeError> {
    // Opus allows zero-length "frames" as an explicit packet-loss/DTX signal (RFC 6716
    // section 3.1) but MF still needs at least the TOC byte to know the packet's frame layout.
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

fn notify_end_streaming(transform: &IMFTransform) {
    unsafe {
        let _: windows::core::Result<()> =
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
    }
}

const fn validate(config: &OpusDecoderConfig) -> Result<(), DecodeError> {
    if config.sample_rate == 0 || config.channels == 0 || config.time_base.den == 0 {
        return Err(DecodeError::InvalidInput);
    }
    Ok(())
}

#[allow(clippy::missing_const_for_fn, reason = "StreamInfo holds Bytes")]
fn stream_info_from(config: &OpusDecoderConfig) -> StreamInfo {
    StreamInfo::Audio {
        id: 0,
        codec: CodecKind::Opus,
        time_base: config.time_base,
        extra_data: Bytes::new(),
        sample_rate: config.sample_rate,
        channels: config.channels,
    }
}

#[cfg(test)]
#[path = "opus_tests.rs"]
mod tests;
