//! `AudioConverter` Opus decode session (raw Opus packets in; Float32 interleaved PCM out). See
//! [ADR-0005](../../../adr/apple/0005-audiotoolbox-opus-decode.md) for why no magic cookie /
//! config record is required at `open()`, unlike `AacDecoder`.
#![allow(unsafe_code)] // real `objc2-*` FFI calls — see `apple/mod.rs`'s doc comment

use std::collections::VecDeque;
use std::ffi::c_void;
use std::ptr::NonNull;

use crate::{AudioDecoder, DecodeError};
use mediaway_common::{AudioFrame, Bytes, CodecKind, Packet, Rational, SampleFormat, StreamInfo};

use objc2_audio_toolbox::{
    AudioConverterComplexInputDataProc, AudioConverterDispose, AudioConverterFillComplexBuffer,
    AudioConverterGetProperty, AudioConverterNew, AudioConverterRef,
    kAudioConverterCurrentInputStreamDescription,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioStreamPacketDescription,
    kAudioFormatFlagIsFloat, kAudioFormatFlagIsPacked, kAudioFormatLinearPCM, kAudioFormatOpus,
};

/// `OSStatus` "no error" value.
const NO_ERROR: i32 = 0;
/// See the companion encoder's identical `STARVATION_STATUS` doc comment.
const STARVATION_STATUS: i32 = 1;
/// Output scratch buffer headroom multiplier over one queried frame — generous, matches the
/// encoder's fixed `OUTPUT_BUF_CAP` reasoning (Opus packets/frames are small).
const MAX_FRAME_SAMPLES_GUESS: u32 = 5760; // 120 ms @ 48 kHz — Opus's own documented max frame size

/// Parameters for opening an [`OpusDecoder`] session — matches
/// `mediaway_decoder::windows::wmf::opus::OpusDecoderConfig`'s exact shape (no `extra_data`
/// field: Opus is self-describing per-packet, see ADR-0005 § Context).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpusDecoderConfig {
    /// Sample rate (Hz).
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u16,
    /// Timestamp timebase for input packets and output frames.
    pub time_base: Rational,
}

impl OpusDecoderConfig {
    /// Config for `sample_rate`/`channels`/`time_base`.
    #[must_use]
    pub const fn new(sample_rate: u32, channels: u16, time_base: Rational) -> Self {
        Self {
            sample_rate,
            channels,
            time_base,
        }
    }
}

/// One raw Opus packet plus the `pts` its decoded PCM frame should carry.
struct PendingPacket {
    payload: Bytes,
    pts: i64,
}

/// Opus decode session over `AudioConverter` (raw packets in, Float32 interleaved PCM out — see
/// ADR-0005 § Scope).
pub struct OpusDecoder {
    converter: AudioConverterRef,
    info: StreamInfo,
    sample_rate: u32,
    channels: u16,
    time_base: Rational,
    /// Output scratch buffer, sized once for the converter's own reported max frame duration.
    output_scratch: Vec<u8>,
    queue: VecDeque<PendingPacket>,
    pending: VecDeque<AudioFrame>,
    flushed: bool,
}

// SAFETY: same reasoning as `aac::AacDecoder`'s identical `unsafe impl Send` — no asynchronous
// callback thread exists for `AudioConverter`.
unsafe impl Send for OpusDecoder {}

impl OpusDecoder {
    /// Open an `AudioConverter` Opus decode session for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::Backend`] on `AudioConverter` failure.
    pub fn open(config: &OpusDecoderConfig) -> Result<Self, DecodeError> {
        validate(config)?;

        let source = AudioStreamBasicDescription {
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
        let destination = AudioStreamBasicDescription {
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

        let mut converter: AudioConverterRef = std::ptr::null_mut();
        // SAFETY: `source`/`destination` are valid, fully-initialized `AudioStreamBasicDescription`
        // values, live for this call (stack locals borrowed via `NonNull::from`); `converter`
        // starts null. Unlike `AacDecoder`, no property is set before use — Opus needs no magic
        // cookie (ADR-0005 § Decision).
        let status = unsafe {
            AudioConverterNew(
                NonNull::from(&source),
                NonNull::from(&destination),
                NonNull::from(&mut converter),
            )
        };
        if status != NO_ERROR || converter.is_null() {
            return Err(DecodeError::Backend);
        }

        let frame_samples = query_frame_samples(converter).unwrap_or(MAX_FRAME_SAMPLES_GUESS);
        let scratch_len = (frame_samples.max(MAX_FRAME_SAMPLES_GUESS) as usize)
            * usize::from(config.channels)
            * 4;

        Ok(Self {
            converter,
            info: stream_info_from(config),
            sample_rate: config.sample_rate,
            channels: config.channels,
            time_base: config.time_base,
            output_scratch: vec![0u8; scratch_len],
            queue: VecDeque::new(),
            pending: VecDeque::new(),
            flushed: false,
        })
    }

    /// Pull as many complete PCM frames as the queued Opus packets allow.
    fn decode_ready_frames(&mut self) -> Result<(), DecodeError> {
        loop {
            if self.queue.is_empty() {
                break;
            }

            let mut ctx = InputContext {
                queue: &mut self.queue,
                consumed: false,
                channels: u32::from(self.channels),
                packet_desc: AudioStreamPacketDescription {
                    mStartOffset: 0,
                    mVariableFramesInPacket: 0,
                    mDataByteSize: 0,
                },
            };

            let output_buffer = AudioBuffer {
                mNumberChannels: u32::from(self.channels),
                mDataByteSize: u32::try_from(self.output_scratch.len()).unwrap_or(u32::MAX),
                mData: self.output_scratch.as_mut_ptr().cast::<c_void>(),
            };
            // See the companion encoder's identical comment: `AudioBufferList` has a private
            // zero-sized marker field, so it must be built via `MaybeUninit` + raw-pointer field
            // writes rather than a struct literal.
            let mut output_list = std::mem::MaybeUninit::<AudioBufferList>::uninit();
            let list_ptr = output_list.as_mut_ptr();
            // SAFETY: `list_ptr` is a valid, properly aligned pointer for a full
            // `AudioBufferList` from a stack-local `MaybeUninit`; `addr_of_mut!` only computes
            // field offsets, never forms a reference to the not-yet-initialized value.
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
            let max_frames =
                u32::try_from(self.output_scratch.len() / (usize::from(self.channels) * 4).max(1))
                    .unwrap_or(u32::MAX);
            let mut io_output_packet_size: u32 = max_frames;

            let input_proc: AudioConverterComplexInputDataProc = Some(input_proc);
            // SAFETY: `self.converter` is a valid, open `AudioConverterRef`; `input_proc` is a
            // real `extern "C-unwind" fn` matching `AudioConverterComplexInputDataProc`'s exact
            // signature; `ctx` is a valid, stack-local value for this call's whole (synchronous,
            // same-thread, nested) duration; `output_list`'s one buffer points at
            // `self.output_scratch`; `packet_desc` is a valid single-slot out-array.
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

            if io_output_packet_size == 0 {
                break;
            }

            let frames = io_output_packet_size;
            let len = usize::try_from(output_list.mBuffers[0].mDataByteSize).unwrap_or(0);
            if len == 0 || len > self.output_scratch.len() {
                return Err(DecodeError::Backend);
            }
            let data = Bytes::copy_from_slice(&self.output_scratch[..len]);

            let pts = if ctx.consumed {
                self.queue.pop_front().map_or(0, |p| p.pts)
            } else {
                0
            };
            let duration = duration_ticks(frames, self.sample_rate, self.time_base);
            self.pending.push_back(AudioFrame {
                pts,
                duration,
                sample_rate: self.sample_rate,
                channels: self.channels,
                format: SampleFormat::F32,
                data,
            });

            if status != NO_ERROR {
                break;
            }
        }
        Ok(())
    }
}

impl AudioDecoder for OpusDecoder {
    fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    fn push_packet(&mut self, packet: &Packet) -> Result<(), DecodeError> {
        if self.flushed {
            return Err(DecodeError::Closed);
        }
        if packet.is_discard {
            return Ok(());
        }
        if packet.payload.is_empty() {
            return Err(DecodeError::InvalidInput);
        }
        self.queue.push_back(PendingPacket {
            // clone: `Bytes` is a shared, refcounted buffer — a cheap refcount bump, not a
            // payload copy, needed because `packet` is borrowed but this queue must outlive it.
            payload: packet.payload.clone(),
            pts: packet.pts,
        });
        self.decode_ready_frames()
    }

    fn poll_frame(&mut self) -> Result<Option<AudioFrame>, DecodeError> {
        Ok(self.pending.pop_front())
    }

    fn flush(&mut self) -> Result<(), DecodeError> {
        if self.flushed {
            return Ok(());
        }
        self.flushed = true;
        Ok(())
    }
}

impl Drop for OpusDecoder {
    fn drop(&mut self) {
        if !self.converter.is_null() {
            // SAFETY: `self.converter` is a valid, owned `AudioConverterRef` created by this
            // session's own `AudioConverterNew` call, disposed exactly once here.
            let _ = unsafe { AudioConverterDispose(self.converter) };
        }
    }
}

struct InputContext<'a> {
    queue: &'a mut VecDeque<PendingPacket>,
    consumed: bool,
    channels: u32,
    packet_desc: AudioStreamPacketDescription,
}

/// `AudioConverterComplexInputDataProc` — identical contract to `aac::input_proc`; supplies
/// **one** raw Opus packet per invocation with its own `AudioStreamPacketDescription`.
///
/// # Safety
///
/// `in_user_data` must be exactly the `&mut InputContext` pointer
/// [`OpusDecoder::decode_ready_frames`] passed as `in_input_data_proc_user_data` for the one
/// `AudioConverterFillComplexBuffer` call this callback is nested inside.
unsafe extern "C-unwind" fn input_proc(
    _in_audio_converter: AudioConverterRef,
    io_number_data_packets: NonNull<u32>,
    io_data: NonNull<AudioBufferList>,
    out_data_packet_description: *mut *mut AudioStreamPacketDescription,
    in_user_data: *mut c_void,
) -> i32 {
    // SAFETY: per this function's own safety contract above.
    let ctx = unsafe { &mut *(in_user_data.cast::<InputContext>()) };
    if ctx.consumed {
        // SAFETY: `io_number_data_packets` is a valid, callback-scoped out-pointer.
        unsafe {
            *io_number_data_packets.as_ptr() = 0;
        }
        return STARVATION_STATUS;
    }
    let Some(packet) = ctx.queue.front() else {
        // SAFETY: same reasoning as above.
        unsafe {
            *io_number_data_packets.as_ptr() = 0;
        }
        return STARVATION_STATUS;
    };

    ctx.packet_desc = AudioStreamPacketDescription {
        mStartOffset: 0,
        mVariableFramesInPacket: 0,
        mDataByteSize: u32::try_from(packet.payload.len()).unwrap_or(0),
    };
    let buffer = AudioBuffer {
        mNumberChannels: ctx.channels,
        mDataByteSize: ctx.packet_desc.mDataByteSize,
        // SAFETY: see `aac::input_proc`'s identical comment — the converter only reads from a
        // caller-supplied input buffer, never writes to it, despite the C signature's `*mut`;
        // this backend never pops the front packet until after the call returns.
        mData: packet.payload.as_ptr().cast_mut().cast::<c_void>(),
    };
    // SAFETY: `io_data`/`io_number_data_packets` are valid, callback-scoped out-pointers;
    // `out_data_packet_description`, when non-null, is a valid out-pointer this backend fills
    // with `&mut ctx.packet_desc` — a stable address for the remainder of this call.
    unsafe {
        (*io_data.as_ptr()).mNumberBuffers = 1;
        (*io_data.as_ptr()).mBuffers[0] = buffer;
        *io_number_data_packets.as_ptr() = 1;
        if !out_data_packet_description.is_null() {
            *out_data_packet_description = std::ptr::addr_of_mut!(ctx.packet_desc);
        }
    }
    ctx.consumed = true;
    NO_ERROR
}

/// Reads back the converter's resolved **source** (compressed Opus) `mFramesPerPacket` via
/// `kAudioConverterCurrentInputStreamDescription` — best-effort, `None` on any failure or a
/// still-unresolved (`0`) value (plausible before any packet has been submitted; the converter
/// may not know the real per-packet frame count until it has seen data). The caller falls back
/// to [`MAX_FRAME_SAMPLES_GUESS`] for scratch-buffer sizing rather than treating this as a fatal
/// open error, unlike the encoder's identical-shaped query, since a decode-side scratch buffer
/// just needs to be *large enough*, not exact. Querying the **output** (destination) side here
/// instead would be meaningless — this backend's destination ASBD hardcodes `mFramesPerPacket: 1`
/// (linear PCM's own fixed one-frame-per-packet convention), which would just echo `1` back.
fn query_frame_samples(converter: AudioConverterRef) -> Option<u32> {
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
    // sized out-buffer, `size` set to its exact byte size per `AudioConverterGetProperty`'s
    // documented contract.
    let status = unsafe {
        AudioConverterGetProperty(
            converter,
            kAudioConverterCurrentInputStreamDescription,
            NonNull::from(&mut size),
            NonNull::from(&mut desc).cast(),
        )
    };
    (status == NO_ERROR && desc.mFramesPerPacket > 0).then_some(desc.mFramesPerPacket)
}

/// Ticks (in `time_base` units) for `frames` PCM frames at `sample_rate` — mirrors
/// `aac::duration_ticks` exactly.
fn duration_ticks(frames: u32, sample_rate: u32, time_base: Rational) -> u64 {
    if sample_rate == 0 || time_base.num == 0 {
        return 0;
    }
    let numerator = u128::from(frames) * u128::from(time_base.den);
    let denominator = u128::from(sample_rate) * u128::from(time_base.num);
    if denominator == 0 {
        return 0;
    }
    u64::try_from(numerator / denominator).unwrap_or(u64::MAX)
}

fn validate(config: &OpusDecoderConfig) -> Result<(), DecodeError> {
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
