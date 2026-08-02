//! AAC encode session: sync inbox WMF AAC MFT (PCM in).

#![allow(unsafe_code)]

use std::collections::VecDeque;

use mediaway_common::{AudioFrame, Bytes, CodecKind, Packet, SampleFormat, StreamInfo};
use mediaway_encoder::{AudioEncoder, AudioEncoderConfig, EncodeError};
use windows::Win32::Media::MediaFoundation::{
    AACMFTEncoder, IMFMediaBuffer, IMFSample, IMFTransform, MF_MT_AAC_PAYLOAD_TYPE,
    MF_MT_AUDIO_AVG_BYTES_PER_SECOND, MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_BLOCK_ALIGNMENT,
    MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
    MF_MT_USER_DATA, MFAudioFormat_AAC, MFAudioFormat_PCM, MFCreateMediaType, MFCreateMemoryBuffer,
    MFCreateSample, MFMediaType_Audio, MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_NOTIFY_END_OF_STREAM,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};

use super::runtime::to_hns;
use super::shared::{Drain, begin_streaming, output_buffer_hint, process_one_output};

/// AAC encode session (PCM upload).
pub(crate) struct WmfAacEncoder {
    transform: IMFTransform,
    info: StreamInfo,
    time_base_num: u64,
    time_base_den: u32,
    sample_rate: u32,
    channels: u16,
    bytes_per_sample: u16,
    block_align: u32,
    output_buf_size: u32,
    pending: VecDeque<Packet>,
    flushed: bool,
}

impl WmfAacEncoder {
    /// Open a WMF AAC encoder for `config`.
    pub(crate) fn open(config: &AudioEncoderConfig) -> Result<Self, EncodeError> {
        validate(config)?;
        super::runtime::ensure_mf()?;

        // SAFETY: inbox sync AAC MFT.
        let transform: IMFTransform =
            unsafe { CoCreateInstance(&AACMFTEncoder, None, CLSCTX_INPROC_SERVER) }
                .map_err(|_| EncodeError::Backend)?;

        let bitrate = if config.bitrate_bps == 0 {
            128_000
        } else {
            config.bitrate_bps
        };
        let (bytes_per_sample, bits_per_sample) = pcm_layout(config.sample_format)?;
        let block_align = u32::from(config.channels) * u32::from(bytes_per_sample);

        configure_types(
            &transform,
            config.sample_rate,
            config.channels,
            bits_per_sample,
            block_align,
            bitrate,
        )?;
        begin_streaming(&transform)?;
        let output_buf_size = output_buffer_hint(&transform)?;

        let mut enc = Self {
            transform,
            info: stream_info_from(config),
            time_base_num: config.time_base.num,
            time_base_den: config.time_base.den,
            sample_rate: config.sample_rate,
            channels: config.channels,
            bytes_per_sample,
            block_align,
            output_buf_size,
            pending: VecDeque::new(),
            flushed: false,
        };
        enc.refresh_extradata();
        Ok(enc)
    }

    fn refresh_extradata(&mut self) {
        let Ok(mt) = (unsafe { self.transform.GetOutputCurrentType(0) }) else {
            return;
        };
        let Ok(blob_size) = (unsafe { mt.GetBlobSize(&MF_MT_USER_DATA) }) else {
            return;
        };
        if blob_size == 0 {
            return;
        }
        let mut buf = vec![0u8; blob_size as usize];
        let mut written = 0u32;
        if unsafe {
            mt.GetBlob(
                &MF_MT_USER_DATA,
                &mut buf,
                Some(std::ptr::from_mut(&mut written)),
            )
        }
        .is_ok()
            && written > 0
        {
            buf.truncate(written as usize);
            if let Some(asc) = asc_from_waveformatex(&buf) {
                if let StreamInfo::Audio { extra_data, .. } = &mut self.info {
                    *extra_data = Bytes::from(asc);
                }
            }
        }
    }

    fn upload_pcm(&self, frame: &AudioFrame) -> Result<IMFSample, EncodeError> {
        let pcm = pcm_bytes(frame, self.bytes_per_sample)?;
        if pcm.is_empty() {
            return Err(EncodeError::InvalidInput);
        }
        if pcm.len() % usize::try_from(self.block_align).unwrap_or(1) != 0 {
            return Err(EncodeError::InvalidInput);
        }
        let pcm_len = u32::try_from(pcm.len()).map_err(|_| EncodeError::InvalidInput)?;
        let sample: IMFSample = unsafe { MFCreateSample() }.map_err(|_| EncodeError::Backend)?;
        let buffer: IMFMediaBuffer =
            unsafe { MFCreateMemoryBuffer(pcm_len) }.map_err(|_| EncodeError::Backend)?;
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
                .map_err(|_| EncodeError::Backend)?;
            if ptr.is_null() || max_len < pcm_len {
                let _: windows::core::Result<()> = buffer.Unlock();
                return Err(EncodeError::Backend);
            }
            std::ptr::copy_nonoverlapping(pcm.as_ptr(), ptr, pcm.len());
            buffer
                .SetCurrentLength(pcm_len)
                .map_err(|_| EncodeError::Backend)?;
            buffer.Unlock().map_err(|_| EncodeError::Backend)?;
        }
        unsafe { sample.AddBuffer(&buffer) }.map_err(|_| EncodeError::Backend)?;

        let block = usize::try_from(self.block_align).map_err(|_| EncodeError::InvalidInput)?;
        let samples = pcm.len() / block;
        let dur_hns = audio_duration_hns(samples, self.sample_rate);
        let hns = to_hns(frame.pts, self.time_base_num, self.time_base_den);
        unsafe {
            sample
                .SetSampleTime(hns)
                .map_err(|_| EncodeError::Backend)?;
            sample
                .SetSampleDuration(dur_hns)
                .map_err(|_| EncodeError::Backend)?;
        }
        Ok(sample)
    }

    fn drain_output(&mut self) -> Result<(), EncodeError> {
        loop {
            match process_one_output(&self.transform, self.output_buf_size, false, &self.info)? {
                Drain::Packet(p) => self.pending.push_back(p),
                Drain::NeedMore => break,
                Drain::StreamChange => self.refresh_extradata(),
            }
        }
        Ok(())
    }
}

impl AudioEncoder for WmfAacEncoder {
    fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    fn push_frame(&mut self, frame: &AudioFrame) -> Result<(), EncodeError> {
        if self.flushed {
            return Err(EncodeError::Closed);
        }
        if frame.sample_rate != self.sample_rate || frame.channels != self.channels {
            return Err(EncodeError::InvalidInput);
        }
        let sample = self.upload_pcm(frame)?;
        unsafe { self.transform.ProcessInput(0, &sample, 0) }.map_err(|_| EncodeError::Backend)?;
        self.drain_output()?;
        if self.info.extra_data().is_empty() {
            self.refresh_extradata();
        }
        Ok(())
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        Ok(self.pending.pop_front())
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        if self.flushed {
            return Ok(());
        }
        self.flushed = true;
        unsafe {
            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)
                .map_err(|_| EncodeError::Backend)?;
            self.transform
                .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)
                .map_err(|_| EncodeError::Backend)?;
        }
        self.drain_output()?;
        super::shared::notify_end_streaming(&self.transform);
        Ok(())
    }
}

fn configure_types(
    transform: &IMFTransform,
    sample_rate: u32,
    channels: u16,
    bits_per_sample: u32,
    block_align: u32,
    bitrate_bps: u32,
) -> Result<(), EncodeError> {
    let out_type = unsafe { MFCreateMediaType() }.map_err(|_| EncodeError::Backend)?;
    unsafe {
        out_type
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
            .map_err(|_| EncodeError::Backend)?;
        out_type
            .SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)
            .map_err(|_| EncodeError::Backend)?;
        out_type
            .SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, u32::from(channels))
            .map_err(|_| EncodeError::Backend)?;
        out_type
            .SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, sample_rate)
            .map_err(|_| EncodeError::Backend)?;
        out_type
            .SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, bitrate_bps / 8)
            .map_err(|_| EncodeError::Backend)?;
        // Raw AAC payload (no ADTS) for MP4-friendly elementary stream.
        out_type
            .SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 0)
            .map_err(|_| EncodeError::Backend)?;
        transform
            .SetOutputType(0, &out_type, 0)
            .map_err(|_| EncodeError::Backend)?;
    }

    let in_type = unsafe { MFCreateMediaType() }.map_err(|_| EncodeError::Backend)?;
    unsafe {
        in_type
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
            .map_err(|_| EncodeError::Backend)?;
        in_type
            .SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)
            .map_err(|_| EncodeError::Backend)?;
        in_type
            .SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, u32::from(channels))
            .map_err(|_| EncodeError::Backend)?;
        in_type
            .SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, sample_rate)
            .map_err(|_| EncodeError::Backend)?;
        in_type
            .SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, bits_per_sample)
            .map_err(|_| EncodeError::Backend)?;
        in_type
            .SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, block_align)
            .map_err(|_| EncodeError::Backend)?;
        transform
            .SetInputType(0, &in_type, 0)
            .map_err(|_| EncodeError::Backend)?;
    }
    Ok(())
}

fn validate(config: &AudioEncoderConfig) -> Result<(), EncodeError> {
    if config.codec != CodecKind::Aac {
        return Err(EncodeError::Unsupported);
    }
    if config.sample_rate == 0 || config.channels == 0 || config.time_base.den == 0 {
        return Err(EncodeError::InvalidInput);
    }
    pcm_layout(config.sample_format)?;
    Ok(())
}

const fn pcm_layout(fmt: SampleFormat) -> Result<(u16, u32), EncodeError> {
    match fmt {
        SampleFormat::S16 | SampleFormat::F32 => Ok((2, 16)),
        SampleFormat::S32 => Ok((4, 32)),
        _ => Err(EncodeError::Unsupported),
    }
}

fn pcm_bytes(frame: &AudioFrame, bytes_per_sample: u16) -> Result<Vec<u8>, EncodeError> {
    match frame.format {
        SampleFormat::S16 if bytes_per_sample == 2 => Ok(frame.data.to_vec()),
        SampleFormat::S32 if bytes_per_sample == 4 => Ok(frame.data.to_vec()),
        SampleFormat::F32 => {
            if frame.data.len() % 4 != 0 {
                return Err(EncodeError::InvalidInput);
            }
            let mut out = Vec::with_capacity(frame.data.len() / 2);
            for chunk in frame.data.chunks_exact(4) {
                let f = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                let clamped = f.clamp(-1.0, 1.0);
                let scaled = (f64::from(clamped) * 32_767.0).round();
                let s = if scaled >= f64::from(i16::MAX) {
                    i16::MAX
                } else if scaled <= f64::from(i16::MIN) {
                    i16::MIN
                } else {
                    #[allow(
                        clippy::cast_possible_truncation,
                        reason = "scaled clamped to i16 range"
                    )]
                    {
                        scaled as i16
                    }
                };
                out.extend_from_slice(&s.to_le_bytes());
            }
            Ok(out)
        }
        _ => Err(EncodeError::Unsupported),
    }
}

fn audio_duration_hns(samples: usize, sample_rate: u32) -> i64 {
    if sample_rate == 0 || samples == 0 {
        return 1;
    }
    let samples_i64 = i64::try_from(samples).unwrap_or(i64::MAX);
    i64::try_from((i128::from(samples_i64) * 10_000_000) / i128::from(sample_rate))
        .unwrap_or(1)
        .max(1)
}

/// Extract `AudioSpecificConfig` bytes from a `WAVEFORMATEX` blob in `MF_MT_USER_DATA`.
fn asc_from_waveformatex(blob: &[u8]) -> Option<Vec<u8>> {
    // WAVEFORMATEX is 18 bytes; AAC uses WAVEFORMATEX + cbSize extension (HEAACWAVEINFO etc.).
    if blob.len() < 20 {
        return None;
    }
    let cb_size = u16::from_le_bytes([blob[16], blob[17]]) as usize;
    if cb_size < 2 || blob.len() < 18 + cb_size {
        return None;
    }
    Some(blob[18..18 + cb_size].to_vec())
}

#[allow(clippy::missing_const_for_fn, reason = "StreamInfo holds Bytes")]
fn stream_info_from(config: &AudioEncoderConfig) -> StreamInfo {
    StreamInfo::Audio {
        id: 0,
        codec: CodecKind::Aac,
        time_base: config.time_base,
        extra_data: Bytes::new(),
        sample_rate: config.sample_rate,
        channels: config.channels,
    }
}
