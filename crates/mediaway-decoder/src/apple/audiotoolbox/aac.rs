//! `AudioConverter` AAC-LC decode session (raw, non-ADTS AAC in; Float32 interleaved PCM out).
//! See [ADR-0004](../../../adr/apple/0004-audiotoolbox-aac-decode.md) for the pull-based
//! `AudioConverterFillComplexBuffer` callback contract, the decompression magic-cookie
//! requirement, and the "raw, not ADTS" input-shape assumption this module relies on.
#![allow(unsafe_code)] // real `objc2-*` FFI calls — see `apple/mod.rs`'s doc comment

use std::collections::VecDeque;
use std::ffi::c_void;
use std::ptr::NonNull;

use crate::{AudioDecoder, DecodeError};
use mediaway_common::{AudioFrame, Bytes, CodecKind, Packet, Rational, SampleFormat, StreamInfo};

use objc2_audio_toolbox::{
    AudioConverterComplexInputDataProc, AudioConverterDispose, AudioConverterFillComplexBuffer,
    AudioConverterNew, AudioConverterRef, AudioConverterSetProperty,
    kAudioConverterDecompressionMagicCookie,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioStreamPacketDescription,
    kAudioFormatFlagIsFloat, kAudioFormatFlagIsPacked, kAudioFormatLinearPCM, kAudioFormatMPEG4AAC,
};

/// `OSStatus` "no error" value.
const NO_ERROR: i32 = 0;
/// This backend's own "no more input" sentinel — see the companion encoder's identical
/// `STARVATION_STATUS` doc comment for the full callback-contract citation this relies on.
const STARVATION_STATUS: i32 = 1;
/// AAC-LC frames are always 1024 samples per channel — matches the companion encoder's
/// `AAC_FRAME_SAMPLES`.
const AAC_FRAME_SAMPLES: u32 = 1024;

/// Parameters for opening an [`AacDecoder`] session — this crate has no shared audio-decode
/// config type (every existing backend defines its own, see ADR-0004 § Context), matching
/// `mediaway_decoder::windows::OpusDecoderConfig`'s identical precedent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AacDecoderConfig {
    /// Sample rate (Hz).
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u16,
    /// Timestamp timebase for input packets and output frames.
    pub time_base: Rational,
    /// Raw `AudioSpecificConfig` bytes — **required**, non-empty (see ADR-0004 § Decision:
    /// `AudioConverter` needs it to resolve the AAC profile/sample-rate table before it can
    /// decode anything, the same "container must supply the config record" constraint this
    /// crate's VP9/AV1 video decode already established).
    pub extra_data: Bytes,
}

impl AacDecoderConfig {
    /// Build a config for `sample_rate`/`channels`/`time_base`/`extra_data` (the raw ASC).
    #[must_use]
    pub const fn new(
        sample_rate: u32,
        channels: u16,
        time_base: Rational,
        extra_data: Bytes,
    ) -> Self {
        Self {
            sample_rate,
            channels,
            time_base,
            extra_data,
        }
    }
}

/// One raw AAC packet plus the `pts` its decoded PCM frame should carry.
struct PendingPacket {
    payload: Bytes,
    pts: i64,
}

/// AAC-LC decode session over `AudioConverter` (raw, non-ADTS AAC input only — see
/// ADR-0004 § Scope).
pub struct AacDecoder {
    converter: AudioConverterRef,
    info: StreamInfo,
    sample_rate: u32,
    channels: u16,
    time_base: Rational,
    /// Output scratch buffer, sized once for `AAC_FRAME_SAMPLES` frames at `channels` — reused
    /// across every `decode_ready_frames` iteration.
    output_scratch: Vec<u8>,
    queue: VecDeque<PendingPacket>,
    pending: VecDeque<AudioFrame>,
    flushed: bool,
}

// SAFETY: same reasoning as the companion encoder's identical `unsafe impl Send` — no
// asynchronous callback thread exists for `AudioConverter`; `AudioConverterFillComplexBuffer`
// and its input callback are both synchronous, nested on the calling thread (see ADR-0004 §
// Context, shared with the encoder ADR).
unsafe impl Send for AacDecoder {}

impl AacDecoder {
    /// Open an `AudioConverter` AAC-LC decode session for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::Unsupported`] when `config.extra_data` is empty (no `AudioSpecificConfig`
    /// to resolve the AAC format with), or [`DecodeError::Backend`] on `AudioConverter` failure.
    pub fn open(config: &AacDecoderConfig) -> Result<Self, DecodeError> {
        validate(config)?;

        let source = AudioStreamBasicDescription {
            mSampleRate: f64::from(config.sample_rate),
            mFormatID: kAudioFormatMPEG4AAC,
            mFormatFlags: 0,
            mBytesPerPacket: 0,
            mFramesPerPacket: AAC_FRAME_SAMPLES,
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
        // starts null.
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

        let mut cookie = config.extra_data.to_vec();
        // SAFETY: `converter` is a valid, just-created `AudioConverterRef`;
        // `kAudioConverterDecompressionMagicCookie`'s documented value is the raw
        // `AudioSpecificConfig` bytes, matching `cookie`'s own shape (`config.extra_data`,
        // required non-empty by `validate` above); `cookie` is a valid, live buffer for this
        // one synchronous call.
        let status = unsafe {
            AudioConverterSetProperty(
                converter,
                kAudioConverterDecompressionMagicCookie,
                u32::try_from(cookie.len()).unwrap_or(0),
                NonNull::new(cookie.as_mut_ptr())
                    .ok_or(DecodeError::InvalidInput)?
                    .cast(),
            )
        };
        if status != NO_ERROR {
            // SAFETY: `converter` was successfully created above and has not been disposed yet;
            // this is the only path that would otherwise leak it before `Drop` ever runs.
            let _ = unsafe { AudioConverterDispose(converter) };
            return Err(DecodeError::Backend);
        }

        let scratch_len = (AAC_FRAME_SAMPLES as usize) * usize::from(config.channels) * 4;

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

    /// Pull as many complete PCM frames as the queued AAC packets allow.
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
            let mut io_output_packet_size: u32 = AAC_FRAME_SAMPLES;

            let input_proc: AudioConverterComplexInputDataProc = Some(input_proc);
            // SAFETY: `self.converter` is a valid, open `AudioConverterRef`; `input_proc` is a
            // real `extern "C-unwind" fn` matching `AudioConverterComplexInputDataProc`'s exact
            // signature; `ctx` is a valid, stack-local value for this call's whole (synchronous,
            // same-thread, nested) duration; `output_list`'s one buffer points at
            // `self.output_scratch`, sized for `AAC_FRAME_SAMPLES` frames; `packet_desc` is a
            // valid single-slot out-array.
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
                // No PCM produced this round — starvation (queue drained) or a real error;
                // either way, stop and wait for more `push_packet` input.
                break;
            }

            let frames = io_output_packet_size;
            // `AudioConverterFillComplexBuffer`'s own doc: "On exit, mDataByteSize is set to the
            // number of bytes written" — read the real count back from `output_list` (the exact
            // memory the C call wrote through), not a value re-derived independently.
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
                // Output was produced, but the callback also signaled starvation on its last
                // invocation this round — nothing more to drain right now.
                break;
            }
        }
        Ok(())
    }
}

impl AudioDecoder for AacDecoder {
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
            // clone: `Bytes` is a shared, refcounted buffer — this is a cheap refcount bump,
            // not a payload copy, needed because `packet` is borrowed but this queue must
            // outlive the call.
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
        // Any queued AAC packets that never accumulated enough to satisfy a full
        // `AAC_FRAME_SAMPLES` output request are not decoded — a documented, deliberate scope
        // cut (see ADR-0004 § Scope), not silently guessed at.
        Ok(())
    }
}

impl Drop for AacDecoder {
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

/// `AudioConverterComplexInputDataProc` — synchronous, nested inside the still-executing
/// `AudioConverterFillComplexBuffer` call that invoked it (see ADR-0004 § Context). Supplies
/// **one** raw AAC packet per invocation with its own `AudioStreamPacketDescription` — the
/// documented shape for compressed, variable-packet-size input.
///
/// # Safety
///
/// `in_user_data` must be exactly the `&mut InputContext` pointer
/// [`AacDecoder::decode_ready_frames`] passed as `in_input_data_proc_user_data` for the one
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
    // Only ever supply the front packet once per `AudioConverterFillComplexBuffer` call — this
    // backend's queue-popping in `decode_ready_frames` happens only after that call returns.
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
        // SAFETY: `packet.payload` borrows `ctx.queue`'s front element, itself borrowed from
        // `AacDecoder::queue` for the whole duration of the enclosing
        // `AudioConverterFillComplexBuffer` call — per that API's documented contract, the
        // converter only reads from a caller-supplied input buffer, never writes to it, despite
        // the C signature's `*mut` (a common C-API constness gap, not a real mutation); this
        // backend never pops the front packet until after the call returns (see
        // `AacDecoder::decode_ready_frames`).
        mData: packet.payload.as_ptr().cast_mut().cast::<c_void>(),
    };
    // SAFETY: `io_data`/`io_number_data_packets` are valid, callback-scoped out-pointers;
    // writing one `AudioBuffer` into `io_data`'s single `mBuffers` slot matches
    // `mNumberBuffers = 1`; `out_data_packet_description`, when non-null, is a valid out-pointer
    // this backend fills with `&mut ctx.packet_desc` — a stable address for the remainder of
    // this call (part of `ctx`, which outlives it).
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

/// Ticks (in `time_base` units) for `frames` PCM frames at `sample_rate` — mirrors the
/// companion encoder's identical `duration_ticks` helper.
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

fn validate(config: &AacDecoderConfig) -> Result<(), DecodeError> {
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
        extra_data: config.extra_data.clone(), // clone: owned StreamInfo snapshot at open
        sample_rate: config.sample_rate,
        channels: config.channels,
    }
}
