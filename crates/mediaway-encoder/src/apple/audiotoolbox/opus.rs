//! `AudioConverter` Opus encode session (Float32 interleaved PCM in). See
//! [ADR-0005](../../../adr/apple/0005-audiotoolbox-opus-encode.md) for why frame duration is
//! converter-chosen (queried, not requested) unlike `mediaway-sw`'s `SwOpusAudioEncoder`.
#![allow(unsafe_code)] // real `objc2-*` FFI calls — see `apple/mod.rs`'s doc comment

use std::collections::VecDeque;
use std::ffi::c_void;
use std::ptr::NonNull;

use crate::{AudioEncoder, AudioEncoderConfig, EncodeError};
use mediaway_common::{AudioFrame, Bytes, CodecKind, Packet, Rational, SampleFormat, StreamInfo};

use objc2_audio_toolbox::{
    AudioConverterComplexInputDataProc, AudioConverterDispose, AudioConverterFillComplexBuffer,
    AudioConverterGetProperty, AudioConverterNew, AudioConverterRef, AudioConverterSetProperty,
    kAudioConverterCurrentOutputStreamDescription, kAudioConverterEncodeBitRate,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioStreamPacketDescription,
    kAudioFormatFlagIsFloat, kAudioFormatFlagIsPacked, kAudioFormatLinearPCM, kAudioFormatOpus,
};

/// `OSStatus` "no error" value.
const NO_ERROR: i32 = 0;
/// See `aac::STARVATION_STATUS` — identical callback-contract sentinel.
const STARVATION_STATUS: i32 = 1;
/// Output scratch buffer capacity per `AudioConverterFillComplexBuffer` call — Opus packets top
/// out around 1275 bytes (RFC 6716's maximum), so this is generous headroom, matching the AAC
/// encoder's identical constant; rejected as `EncodeError::Backend` if ever exceeded, never
/// silently truncated.
const OUTPUT_BUF_CAP: usize = 8192;
/// Default bitrate when [`AudioEncoderConfig::bitrate_bps`] is `0`.
const DEFAULT_BITRATE_BPS: u32 = 128_000;

/// Opus encode session over `AudioConverter` (Float32 interleaved PCM input only — see
/// ADR-0005 § Scope). Frame duration is whatever the converter itself resolves at `open()`
/// (`frame_samples`), not caller-selectable through this backend.
pub(crate) struct OpusEncoder {
    converter: AudioConverterRef,
    info: StreamInfo,
    sample_rate: u32,
    channels: u16,
    time_base: Rational,
    /// Samples per channel per output packet — queried from
    /// `kAudioConverterCurrentOutputStreamDescription` after `open()`, since Opus (unlike AAC)
    /// has no fixed frame size (see ADR-0005 § Context).
    frame_samples: u32,
    /// Interleaved F32 bytes accumulated from `push_frame`, not yet consumed by the converter.
    pcm: Vec<u8>,
    read_pos: usize,
    samples_consumed: u64,
    first_pts: Option<i64>,
    pending: VecDeque<Packet>,
    flushed: bool,
}

// SAFETY: same reasoning as `aac::AacEncoder`'s identical `unsafe impl Send` — `AudioConverter`
// has no asynchronous callback thread; `AudioConverterFillComplexBuffer` and its input callback
// are both synchronous, nested on the calling thread.
unsafe impl Send for OpusEncoder {}

impl OpusEncoder {
    /// Open an `AudioConverter` Opus encode session for `config`.
    pub(crate) fn open(config: &AudioEncoderConfig) -> Result<Self, EncodeError> {
        validate(config)?;

        let source = AudioStreamBasicDescription {
            mSampleRate: f64::from(config.sample_rate),
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
            mBytesPerPacket: u32::from(config.channels) * 4,
            mFramesPerPacket: 1,
            mBytesPerFrame: u32::from(config.channels) * 4,
            mChannelsPerFrame: u32::from(config.channels),
            mBitsPerChannel: 32,
            mReserved: 0,
        };
        // Compressed-format ASBD: `mFramesPerPacket` is left at 0 (unlike AAC's fixed 1024) —
        // Opus's frame size is converter-chosen, queried back below (ADR-0005 § Decision).
        let destination = AudioStreamBasicDescription {
            mSampleRate: f64::from(config.sample_rate),
            mFormatID: kAudioFormatOpus,
            mFormatFlags: 0,
            mBytesPerPacket: 0,
            mFramesPerPacket: 0,
            mBytesPerFrame: 0,
            mChannelsPerFrame: u32::from(config.channels),
            mBitsPerChannel: 0,
            mReserved: 0,
        };

        let mut converter: AudioConverterRef = std::ptr::null_mut();
        // SAFETY: `source`/`destination` are valid, fully-initialized `AudioStreamBasicDescription`
        // values, live for this call (stack locals borrowed via `NonNull::from`); `converter`
        // starts null and is only read by this function's caller after a `NO_ERROR` status.
        let status = unsafe {
            AudioConverterNew(
                NonNull::from(&source),
                NonNull::from(&destination),
                NonNull::from(&mut converter),
            )
        };
        if status != NO_ERROR || converter.is_null() {
            return Err(EncodeError::Backend);
        }

        let frame_samples = query_frame_samples(converter);
        if frame_samples == 0 {
            // SAFETY: `converter` was successfully created above and has not been disposed yet;
            // this is the only path that would otherwise leak it before `Drop` ever runs.
            let _ = unsafe { AudioConverterDispose(converter) };
            return Err(EncodeError::Backend);
        }

        let bitrate = if config.bitrate_bps == 0 {
            DEFAULT_BITRATE_BPS
        } else {
            config.bitrate_bps
        };
        let mut bitrate_value = bitrate;
        // SAFETY: `converter` is a valid, just-created `AudioConverterRef`;
        // `kAudioConverterEncodeBitRate` is a generic property (not AAC-specific — see ADR-0005),
        // documented value type `UInt32`, matching `bitrate_value`'s size passed below.
        let _ = unsafe {
            AudioConverterSetProperty(
                converter,
                kAudioConverterEncodeBitRate,
                u32::try_from(size_of::<u32>()).unwrap_or(4),
                NonNull::from(&mut bitrate_value).cast(),
            )
        };

        Ok(Self {
            converter,
            info: stream_info_from(config),
            sample_rate: config.sample_rate,
            channels: config.channels,
            time_base: config.time_base,
            frame_samples,
            pcm: Vec::new(),
            read_pos: 0,
            samples_consumed: 0,
            first_pts: None,
            pending: VecDeque::new(),
            flushed: false,
        })
    }

    /// Append PCM, then pull as many complete Opus packets as are ready.
    fn drain_ready_packets(&mut self) -> Result<(), EncodeError> {
        let bytes_per_frame = u32::from(self.channels) * 4;
        loop {
            let mut ctx = InputContext {
                pcm: &self.pcm[self.read_pos..],
                consumed: 0,
                bytes_per_frame,
                channels: u32::from(self.channels),
            };

            let mut output = [0u8; OUTPUT_BUF_CAP];
            let output_buffer = AudioBuffer {
                mNumberChannels: u32::from(self.channels),
                mDataByteSize: u32::try_from(OUTPUT_BUF_CAP).unwrap_or(u32::MAX),
                mData: output.as_mut_ptr().cast::<c_void>(),
            };
            // See `aac::AacEncoder::drain_ready_packets`'s identical comment on why
            // `AudioBufferList` must be built via `MaybeUninit` + raw-pointer field writes.
            let mut output_list = std::mem::MaybeUninit::<AudioBufferList>::uninit();
            let list_ptr = output_list.as_mut_ptr();
            // SAFETY: `list_ptr` is valid, properly aligned pointer for a full
            // `AudioBufferList` (from a stack-local `MaybeUninit`); `addr_of_mut!` never forms a
            // reference to the not-yet-initialized value, only computes field offsets.
            let mut output_list = unsafe {
                std::ptr::addr_of_mut!((*list_ptr).mNumberBuffers).write(1);
                std::ptr::addr_of_mut!((*list_ptr).mBuffers).write([output_buffer]);
                output_list.assume_init()
            };
            let mut packet_desc = AudioStreamPacketDescription {
                mStartOffset: 0,
                mVariableFramesInPacket: 0,
                mDataByteSize: 0,
            };
            let mut io_output_packet_size: u32 = 1;

            let input_proc: AudioConverterComplexInputDataProc = Some(input_proc);
            // SAFETY: `self.converter` is a valid, open `AudioConverterRef`; `input_proc` is a
            // real `extern "C-unwind" fn` matching `AudioConverterComplexInputDataProc`'s exact
            // signature; `ctx` is a valid, stack-local value for this call's whole (synchronous,
            // same-thread, nested) duration; `output_list`'s one buffer points at `output`, a
            // valid `OUTPUT_BUF_CAP`-byte stack array; `packet_desc` is a valid single-slot
            // out-array (`io_output_packet_size` never exceeds `1`).
            let status = unsafe {
                AudioConverterFillComplexBuffer(
                    self.converter,
                    input_proc,
                    std::ptr::from_mut(&mut ctx).cast::<c_void>(),
                    NonNull::from(&mut io_output_packet_size),
                    NonNull::from(&mut output_list),
                    &mut packet_desc,
                )
            };

            self.read_pos += ctx.consumed;
            self.samples_consumed +=
                u64::from(u32::try_from(ctx.consumed).unwrap_or(0) / bytes_per_frame.max(1));

            if io_output_packet_size == 0 {
                break;
            }

            let len = usize::try_from(output_list.mBuffers[0].mDataByteSize).unwrap_or(0);
            if len == 0 || len > OUTPUT_BUF_CAP {
                return Err(EncodeError::Backend);
            }
            let payload = Bytes::copy_from_slice(&output[..len]);

            let pts = self
                .first_pts
                .map_or(0, |first| first + self.output_pts_offset());
            self.pending.push_back(Packet {
                stream_id: 0,
                pts,
                dts: pts,
                duration: duration_ticks(self.frame_samples, self.sample_rate, self.time_base),
                is_keyframe: true,
                is_discard: false,
                payload,
            });

            if status != NO_ERROR {
                break;
            }
        }

        if self.read_pos > 0 {
            self.pcm.drain(..self.read_pos);
            self.read_pos = 0;
        }
        Ok(())
    }

    /// Samples already emitted as complete output packets (including the one just produced this
    /// round), converted to `time_base` ticks — mirrors `AacEncoder::output_pts_offset` with
    /// `frame_samples` in place of the AAC-fixed constant.
    fn output_pts_offset(&self) -> i64 {
        let ticks = duration_ticks(
            u32::try_from(
                self.samples_consumed
                    .saturating_sub(u64::from(self.frame_samples)),
            )
            .unwrap_or(u32::MAX),
            self.sample_rate,
            self.time_base,
        );
        i64::try_from(ticks).unwrap_or(i64::MAX)
    }
}

impl AudioEncoder for OpusEncoder {
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
        if frame.format != SampleFormat::F32 {
            return Err(EncodeError::Unsupported);
        }
        if frame.data.is_empty() {
            return Err(EncodeError::InvalidInput);
        }
        let bytes_per_frame = usize::from(self.channels) * 4;
        if !frame.data.len().is_multiple_of(bytes_per_frame) {
            return Err(EncodeError::InvalidInput);
        }
        if self.first_pts.is_none() {
            self.first_pts = Some(frame.pts);
        }
        self.pcm.extend_from_slice(&frame.data);
        self.drain_ready_packets()
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        Ok(self.pending.pop_front())
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        if self.flushed {
            return Ok(());
        }
        self.flushed = true;
        // Any PCM buffered but short of one more full `frame_samples` Opus frame is not
        // encoded — a documented, deliberate scope cut (see ADR-0005 § Scope), matching the AAC
        // encoder's identical convention.
        Ok(())
    }
}

impl Drop for OpusEncoder {
    fn drop(&mut self) {
        if !self.converter.is_null() {
            // SAFETY: `self.converter` is a valid, owned `AudioConverterRef` created by this
            // session's own `AudioConverterNew` call, disposed exactly once here.
            let _ = unsafe { AudioConverterDispose(self.converter) };
        }
    }
}

struct InputContext<'a> {
    pcm: &'a [u8],
    consumed: usize,
    bytes_per_frame: u32,
    channels: u32,
}

/// `AudioConverterComplexInputDataProc` — identical contract to `aac::input_proc`; see that
/// function's doc comment for the full citation.
///
/// # Safety
///
/// `in_user_data` must be exactly the `&mut InputContext` pointer
/// [`OpusEncoder::drain_ready_packets`] passed as `in_input_data_proc_user_data` for the one
/// `AudioConverterFillComplexBuffer` call this callback is nested inside.
unsafe extern "C-unwind" fn input_proc(
    _in_audio_converter: AudioConverterRef,
    io_number_data_packets: NonNull<u32>,
    io_data: NonNull<AudioBufferList>,
    _out_data_packet_description: *mut *mut AudioStreamPacketDescription,
    in_user_data: *mut c_void,
) -> i32 {
    // SAFETY: per this function's own safety contract above.
    let ctx = unsafe { &mut *(in_user_data.cast::<InputContext<'_>>()) };
    let remaining = ctx.pcm.get(ctx.consumed..).unwrap_or(&[]);
    let available_frames = if ctx.bytes_per_frame == 0 {
        0
    } else {
        u32::try_from(remaining.len()).unwrap_or(u32::MAX) / ctx.bytes_per_frame
    };
    if available_frames == 0 {
        // SAFETY: `io_number_data_packets` is a valid, callback-scoped out-pointer per
        // `AudioConverterComplexInputDataProc`'s documented contract.
        unsafe {
            *io_number_data_packets.as_ptr() = 0;
        }
        return STARVATION_STATUS;
    }

    // SAFETY: same reasoning as above.
    let requested = unsafe { *io_number_data_packets.as_ptr() };
    let give_frames = requested.min(available_frames);
    let give_bytes = (give_frames * ctx.bytes_per_frame) as usize;
    let Some(slice) = remaining.get(..give_bytes) else {
        unsafe {
            *io_number_data_packets.as_ptr() = 0;
        }
        return STARVATION_STATUS;
    };

    let buffer = AudioBuffer {
        mNumberChannels: ctx.channels,
        mDataByteSize: u32::try_from(slice.len()).unwrap_or(0),
        // SAFETY: see `aac::input_proc`'s identical comment — `AudioConverter` only reads from a
        // caller-supplied input buffer, never writes to it, despite the C signature's `*mut`.
        mData: slice.as_ptr().cast_mut().cast::<c_void>(),
    };
    // SAFETY: `io_data` is a valid, callback-scoped out-pointer; writing one `AudioBuffer` into
    // its single `mBuffers` slot matches this backend's `mNumberBuffers = 1` interleaved-PCM
    // shape.
    unsafe {
        (*io_data.as_ptr()).mNumberBuffers = 1;
        (*io_data.as_ptr()).mBuffers[0] = buffer;
        *io_number_data_packets.as_ptr() = give_frames;
    }
    ctx.consumed += give_bytes;
    NO_ERROR
}

/// Reads back the converter's resolved `mFramesPerPacket` via
/// `kAudioConverterCurrentOutputStreamDescription` — the only locally-grounded way to learn
/// Opus's converter-chosen frame duration (see ADR-0005 § Context). Returns `0` on any failure
/// (treated as a fatal open error by the caller, not a silent fallback guess).
fn query_frame_samples(converter: AudioConverterRef) -> u32 {
    let mut desc = AudioStreamBasicDescription {
        mSampleRate: 0.0,
        mFormatID: 0,
        mFormatFlags: 0,
        mBytesPerPacket: 0,
        mFramesPerPacket: 0,
        mBytesPerFrame: 0,
        mChannelsPerFrame: 0,
        mBitsPerChannel: 0,
        mReserved: 0,
    };
    let mut size = u32::try_from(size_of::<AudioStreamBasicDescription>()).unwrap_or(0);
    // SAFETY: `converter` is a valid, open `AudioConverterRef`; `desc` is a valid, correctly
    // sized out-buffer for `AudioStreamBasicDescription`, `size` set to its exact byte size per
    // `AudioConverterGetProperty`'s "on entry, the size of the memory pointed to by
    // outPropertyData" contract.
    let status = unsafe {
        AudioConverterGetProperty(
            converter,
            kAudioConverterCurrentOutputStreamDescription,
            NonNull::from(&mut size),
            NonNull::from(&mut desc).cast(),
        )
    };
    if status != NO_ERROR {
        return 0;
    }
    desc.mFramesPerPacket
}

/// Ticks (in `time_base` units) for `samples` frames at `sample_rate` — mirrors
/// `aac::duration_ticks` exactly.
fn duration_ticks(samples: u32, sample_rate: u32, time_base: Rational) -> u64 {
    if sample_rate == 0 || time_base.num == 0 {
        return 0;
    }
    let numerator = u128::from(samples) * u128::from(time_base.den);
    let denominator = u128::from(sample_rate) * u128::from(time_base.num);
    if denominator == 0 {
        return 0;
    }
    u64::try_from(numerator / denominator).unwrap_or(u64::MAX)
}

fn validate(config: &AudioEncoderConfig) -> Result<(), EncodeError> {
    if config.codec != CodecKind::Opus {
        return Err(EncodeError::Unsupported);
    }
    if config.sample_rate == 0 || config.channels == 0 || config.time_base.den == 0 {
        return Err(EncodeError::InvalidInput);
    }
    if config.sample_format != SampleFormat::F32 {
        return Err(EncodeError::Unsupported);
    }
    Ok(())
}

#[allow(clippy::missing_const_for_fn, reason = "StreamInfo holds Bytes")]
fn stream_info_from(config: &AudioEncoderConfig) -> StreamInfo {
    StreamInfo::Audio {
        id: 0,
        codec: CodecKind::Opus,
        time_base: config.time_base,
        extra_data: Bytes::new(),
        sample_rate: config.sample_rate,
        channels: config.channels,
    }
}
