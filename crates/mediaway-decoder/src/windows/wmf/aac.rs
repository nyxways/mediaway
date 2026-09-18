//! AAC decode session: inbox WMF AAC decoder MFT (`CMSAACDecMFT`, Float32 PCM out).
//!
//! Input is **raw AAC** — one whole `raw_data_block()` per packet, which is exactly how
//! MP4 stores AAC samples. ADTS is *not* handled here; see § ADTS below.
//!
//! # `MF_MT_USER_DATA` is the whole trick
//!
//! Unlike the Opus decoder MFT (rate + channel count is enough), the AAC decoder MFT
//! cannot configure itself without the stream's `AudioSpecificConfig`. Microsoft's AAC
//! decoder reference
//! (<https://learn.microsoft.com/en-us/windows/win32/medfound/aac-decoder>) specifies
//! `MF_MT_USER_DATA` for `MFAudioFormat_AAC` as "the portion of the `HEAACWAVEINFO`
//! structure that appears after the `WAVEFORMATEX` structure ... followed by the
//! `AudioSpecificConfig()` data":
//!
//! ```text
//! offset  size  field
//!      0     2  wPayloadType                  (0 = raw AAC, 1 = ADTS, 3 = LOAS/LATM)
//!      2     2  wAudioProfileLevelIndication  (0 or 0xFE = unspecified)
//!      4     2  wStructType                   (0)
//!      6     2  wReserved1                    (0)
//!      8     4  dwReserved2                   (0)
//!     12     n  AudioSpecificConfig
//! ```
//!
//! All little-endian. That layout is corroborated in-tree: this workspace's WMF AAC
//! *encoder* reads back the 14-byte blob `[00 00 29 00 00 00 00 00 00 00 00 00 11 90]`
//! for 48 kHz stereo AAC-LC (`mediaway-encoder`'s `windows::wmf::aac`), whose trailing
//! two bytes `11 90` are the canonical `AudioSpecificConfig` for that format.
//!
//! # An empty `extra_data` is an error, not a guess
//!
//! [`AacDecoderConfig::extra_data`] must be non-empty. Synthesizing an
//! `AudioSpecificConfig` from `sample_rate`/`channels` alone would silently produce
//! wrong output for any stream using SBR or PS — for those the media type's rate and
//! channel count describe the *core* stream before the tools are applied, so the pair is
//! not sufficient to reconstruct the config. Failing with
//! [`DecodeError::Unsupported`] matches this crate's Apple AAC decoder
//! (`apple/adr/0004`) and its VP9/AV1 video decoders, which likewise require the
//! container's config record at `open()`.
//!
//! # ADTS
//!
//! The MFT can consume ADTS (`wPayloadType = 1`), but this session always sets `0`.
//! Sniffing the payload per packet would make the input shape implicit; a caller with
//! ADTS should either use the `adts-core` crate to strip headers, or this module should
//! grow an explicit config field. Deliberately not guessed —
//! see [ADR-0006](../../../adr/windows/0006-wmf-aac-decode.md) § Scope.
//!
//! Implements the facade [`crate::AudioDecoder`] trait
//! ([ADR-0003](../../../adr/0003-audio-decoder-trait.md)) in addition to its own inherent
//! methods, matching [`super::opus`].

#![allow(unsafe_code)]

use std::collections::VecDeque;

use crate::{AudioDecoder, DecodeError};
use mediaway_common::{AudioFrame, Bytes, CodecKind, Packet, Rational, SampleFormat, StreamInfo};
use windows::Win32::Media::MediaFoundation::{
    CMSAACDecMFT, IMFTransform, MF_MT_AAC_PAYLOAD_TYPE, MF_MT_AUDIO_NUM_CHANNELS,
    MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_MT_USER_DATA,
    MFAudioFormat_AAC, MFAudioFormat_Float, MFCreateMediaType, MFMediaType_Audio,
    MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_NOTIFY_END_OF_STREAM,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};

use super::audio_mft::{
    Drain, OutputPayload, begin_streaming, notify_end_streaming, output_buffer_size,
    packet_to_sample, process_one_output,
};
use super::runtime::from_hns;

/// Size of the `HEAACWAVEINFO` tail that precedes the `AudioSpecificConfig` in
/// `MF_MT_USER_DATA` — see the module docs for the field-by-field layout.
const HEAAC_WAVEINFO_TAIL_LEN: usize = 12;

/// `wPayloadType` for raw AAC (`raw_data_block()` elements only), which is what MP4 and
/// this session use.
const PAYLOAD_TYPE_RAW_AAC: u16 = 0;

/// `wAudioProfileLevelIndication` "no audio profile specified" (per the MS reference).
/// Used because the profile is already implied by the `AudioSpecificConfig` that follows.
const PROFILE_LEVEL_UNSPECIFIED: u16 = 0xFE;

/// Config for [`WmfAacDecoder::open`].
///
/// Mirrors `mediaway_decoder::apple::AacDecoderConfig`; this crate has no shared audio
/// decode config type (see ADR-0003 § Context).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AacDecoderConfig {
    /// Sample rate (Hz) of the *core* AAC stream, before SBR/PS are applied.
    pub sample_rate: u32,
    /// Channel count of the *core* AAC stream, before PS is applied.
    pub channels: u16,
    /// Stream timebase; audio sessions elsewhere in this workspace use `1 / sample_rate`
    /// so `Packet`/`AudioFrame` `pts`/`duration` are plain sample counts.
    pub time_base: Rational,
    /// Raw `AudioSpecificConfig` bytes — **required**, non-empty. For AAC in MP4 this is
    /// the `esds` descriptor's `DecoderSpecificInfo`, which `iso-bmff` already captures
    /// into `StreamInfo::extra_data`. See the module docs for why this is not optional.
    pub extra_data: Bytes,
}

impl AacDecoderConfig {
    /// Config for `sample_rate`/`channels`/`extra_data` with a `1 / sample_rate` timebase.
    #[must_use]
    pub const fn new(sample_rate: u32, channels: u16, extra_data: Bytes) -> Self {
        Self {
            sample_rate,
            channels,
            time_base: Rational::new(1, sample_rate),
            extra_data,
        }
    }
}

/// AAC decode session (WMF `CMSAACDecMFT`, Float32 PCM output; see module docs).
pub struct WmfAacDecoder {
    transform: IMFTransform,
    info: StreamInfo,
    time_base_num: u64,
    time_base_den: u32,
    channels: u16,
    output_buf_size: u32,
    pending: VecDeque<AudioFrame>,
    flushed: bool,
}

impl WmfAacDecoder {
    /// Open a WMF AAC decoder for `config`.
    ///
    /// # Errors
    ///
    /// [`DecodeError::InvalidInput`] for a zero rate/channel count/timebase,
    /// [`DecodeError::Unsupported`] when `config.extra_data` is empty (no
    /// `AudioSpecificConfig`), or [`DecodeError::Backend`] when the MFT is missing or
    /// rejects the media type.
    pub fn open(config: &AacDecoderConfig) -> Result<Self, DecodeError> {
        validate(config)?;
        super::runtime::ensure_mf()?;

        // SAFETY: inbox sync AAC decoder MFT.
        let transform: IMFTransform =
            unsafe { CoCreateInstance(&CMSAACDecMFT, None, CLSCTX_INPROC_SERVER) }
                .map_err(|_| DecodeError::Backend)?;

        configure_types(
            &transform,
            config.sample_rate,
            config.channels,
            &config.extra_data,
        )?;
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

    /// Stream metadata: AAC audio, the configured timebase, Float32 PCM out.
    pub const fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    /// Submit one raw AAC packet (exactly one `raw_data_block()`, no ADTS header).
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::Closed`] after [`flush`](Self::flush),
    /// [`DecodeError::InvalidInput`] for an empty payload, or [`DecodeError::Backend`]
    /// when the MFT rejects the sample.
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

impl AudioDecoder for WmfAacDecoder {
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

/// Build the `MF_MT_USER_DATA` blob: `HEAACWAVEINFO` tail + `AudioSpecificConfig`.
fn user_data_blob(asc: &[u8]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(HEAAC_WAVEINFO_TAIL_LEN + asc.len());
    blob.extend_from_slice(&PAYLOAD_TYPE_RAW_AAC.to_le_bytes());
    blob.extend_from_slice(&PROFILE_LEVEL_UNSPECIFIED.to_le_bytes());
    blob.extend_from_slice(&0u16.to_le_bytes()); // wStructType
    blob.extend_from_slice(&0u16.to_le_bytes()); // wReserved1
    blob.extend_from_slice(&0u32.to_le_bytes()); // dwReserved2
    debug_assert_eq!(blob.len(), HEAAC_WAVEINFO_TAIL_LEN);
    blob.extend_from_slice(asc);
    blob
}

fn configure_types(
    transform: &IMFTransform,
    sample_rate: u32,
    channels: u16,
    asc: &[u8],
) -> Result<(), DecodeError> {
    let in_type = unsafe { MFCreateMediaType() }.map_err(|_| DecodeError::Backend)?;
    let user_data = user_data_blob(asc);
    unsafe {
        in_type
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
            .map_err(|_| DecodeError::Backend)?;
        in_type
            .SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)
            .map_err(|_| DecodeError::Backend)?;
        in_type
            .SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, u32::from(channels))
            .map_err(|_| DecodeError::Backend)?;
        in_type
            .SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, sample_rate)
            .map_err(|_| DecodeError::Backend)?;
        in_type
            .SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, u32::from(PAYLOAD_TYPE_RAW_AAC))
            .map_err(|_| DecodeError::Backend)?;
        in_type
            .SetBlob(&MF_MT_USER_DATA, &user_data)
            .map_err(|_| DecodeError::Backend)?;
        transform
            .SetInputType(0, &in_type, 0)
            .map_err(|_| DecodeError::Backend)?;
    }

    select_float_output(transform)
}

/// Pick the decoder's own Float32 PCM output proposal.
///
/// Unlike the Opus decoder MFT (which offers exactly one output type), the AAC decoder
/// registers both `MFAudioFormat_Float` and 16-bit `MFAudioFormat_PCM`, so index 0 is not
/// guaranteed to be the one this crate wants. Enumerate and take the float type, keeping
/// every audio backend in this workspace on [`SampleFormat::F32`]; fall back to the first
/// proposal only if no float type is offered, so a future MFT revision degrades rather
/// than fails outright.
fn select_float_output(transform: &IMFTransform) -> Result<(), DecodeError> {
    let mut fallback = None;
    for index in 0..16u32 {
        let Ok(candidate) = (unsafe { transform.GetOutputAvailableType(0, index) }) else {
            break;
        };
        let subtype = unsafe { candidate.GetGUID(&MF_MT_SUBTYPE) };
        if subtype.is_ok_and(|guid| guid == MFAudioFormat_Float) {
            unsafe {
                transform
                    .SetOutputType(0, &candidate, 0)
                    .map_err(|_| DecodeError::Backend)?;
            }
            return Ok(());
        }
        if fallback.is_none() {
            fallback = Some(candidate);
        }
    }
    let chosen = fallback.ok_or(DecodeError::Backend)?;
    unsafe {
        transform
            .SetOutputType(0, &chosen, 0)
            .map_err(|_| DecodeError::Backend)?;
    }
    Ok(())
}

const fn validate(config: &AacDecoderConfig) -> Result<(), DecodeError> {
    if config.sample_rate == 0 || config.channels == 0 || config.time_base.den == 0 {
        return Err(DecodeError::InvalidInput);
    }
    if config.extra_data.is_empty() {
        return Err(DecodeError::Unsupported);
    }
    Ok(())
}

#[allow(clippy::missing_const_for_fn, reason = "StreamInfo holds Bytes")]
fn stream_info_from(config: &AacDecoderConfig) -> StreamInfo {
    StreamInfo::Audio {
        id: 0,
        codec: CodecKind::Aac,
        time_base: config.time_base,
        // clone: owned StreamInfo snapshot at open, matching apple's AacDecoder
        extra_data: config.extra_data.clone(),
        sample_rate: config.sample_rate,
        channels: config.channels,
    }
}

#[cfg(test)]
#[path = "aac_tests.rs"]
mod tests;
