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
//! Implements the facade [`crate::AudioDecoder`] trait ([ADR-0003](../../../adr/0003-audio-decoder-trait.md))
//! in addition to its own inherent methods (kept for callers not importing the trait —
//! see that ADR for why both exist). Not wired into any `WindowsAudioDecoder`-style
//! backend switcher yet — no such type exists (unlike video's `WindowsVideoDecoder`),
//! since Opus is the only Windows audio decode path today. See `docs/roadmap.md`.

#![allow(unsafe_code)]

use std::collections::VecDeque;

use crate::{AudioDecoder, DecodeError};
use mediaway_common::{AudioFrame, Bytes, CodecKind, Packet, Rational, SampleFormat, StreamInfo};
use windows::Win32::Media::MediaFoundation::{
    CMSOpusDecMFT, IMFTransform, MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND,
    MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MFAudioFormat_Opus, MFCreateMediaType, MFMediaType_Audio,
    MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_NOTIFY_END_OF_STREAM,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};

use super::audio_mft::{
    Drain, OutputPayload, begin_streaming, notify_end_streaming, output_buffer_size,
    packet_to_sample, process_one_output,
};
use super::runtime::from_hns;

/// Config for [`WmfOpusDecoder::open`].
pub struct OpusDecoderConfig {
    /// Sample rate (Hz). The WMF decoder MFT's input type is negotiated at
    /// this rate; the output (Float32 PCM) comes back at the same rate.
    pub sample_rate: u32,
    /// Channel count (1 or 2).
    pub channels: u16,
    /// Stream timebase; audio sessions elsewhere in this workspace use
    /// `1 / sample_rate` so `Packet`/`AudioFrame` `pts`/`duration` are plain
    /// sample counts.
    pub time_base: Rational,
}

impl OpusDecoderConfig {
    /// Config for `sample_rate`/`channels` with `1 / sample_rate` timebase.
    #[must_use]
    pub const fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            sample_rate,
            channels,
            time_base: Rational::new(1, sample_rate),
        }
    }
}

/// Opus decode session (WMF `CMSOpusDecMFT`, Float32 PCM output; see module docs).
pub struct WmfOpusDecoder {
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
    pub fn open(config: &OpusDecoderConfig) -> Result<Self, DecodeError> {
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

    /// Stream metadata: Opus audio, `1 / sample_rate` timebase, Float32 out.
    pub const fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    /// Submit one compressed Opus packet (pre-extradata `Packet` payload).
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::Closed`] after [`flush`](Self::flush), or
    /// [`DecodeError::Backend`] when the MFT rejects the sample.
    pub fn push_packet(&mut self, packet: &Packet) -> Result<(), DecodeError> {
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

    /// Pull the next decoded Float32 PCM frame, if any.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::Backend`] when the MFT's output drain fails.
    pub fn poll_frame(&mut self) -> Result<Option<AudioFrame>, DecodeError> {
        if self.pending.is_empty() {
            self.drain_output()?;
        }
        Ok(self.pending.pop_front())
    }

    /// Signal end-of-stream; drains any remaining PCM with
    /// [`poll_frame`](Self::poll_frame).
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::Backend`] when the MFT rejects the drain message.
    pub fn flush(&mut self) -> Result<(), DecodeError> {
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

impl AudioDecoder for WmfOpusDecoder {
    fn stream_info(&self) -> &StreamInfo {
        self.stream_info()
    }

    fn push_packet(&mut self, packet: &Packet) -> Result<(), DecodeError> {
        self.push_packet(packet)
    }

    fn poll_frame(&mut self) -> Result<Option<AudioFrame>, DecodeError> {
        self.poll_frame()
    }

    fn flush(&mut self) -> Result<(), DecodeError> {
        self.flush()
    }
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
