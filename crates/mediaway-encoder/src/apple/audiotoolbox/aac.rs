//! `AudioConverter` AAC-LC encode session (Float32 interleaved PCM in). See
//! [ADR-0004](../../../adr/apple/0004-audiotoolbox-aac-encode.md) for the pull-based
//! `AudioConverterFillComplexBuffer` callback contract this module implements against.
#![allow(unsafe_code)] // real `objc2-*` FFI calls — see `apple/mod.rs`'s doc comment

use std::collections::VecDeque;
use std::ffi::c_void;
use std::ptr::NonNull;

use crate::{AudioEncoder, AudioEncoderConfig, EncodeError};
use mediaway_common::{AudioFrame, Bytes, CodecKind, Packet, Rational, SampleFormat, StreamInfo};

use objc2_audio_toolbox::{
    AudioConverterComplexInputDataProc, AudioConverterDispose, AudioConverterFillComplexBuffer,
    AudioConverterGetProperty, AudioConverterNew, AudioConverterRef, AudioConverterSetProperty,
    kAudioConverterCompressionMagicCookie, kAudioConverterEncodeBitRate,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioStreamPacketDescription,
    kAudioFormatFlagIsFloat, kAudioFormatFlagIsPacked, kAudioFormatLinearPCM, kAudioFormatMPEG4AAC,
};

/// `OSStatus` "no error" value.
const NO_ERROR: i32 = 0;
/// This backend's own "temporarily out of input" sentinel returned by [`input_proc`] — never
/// surfaced to callers, only observed inside [`AacEncoder::drain_ready_packets`]'s loop. Any
/// nonzero value works per `AudioConverterComplexInputDataProc`'s doc contract ("if the callback
/// returns an error, it must return zero packets of data... this mechanism can be used when an
/// input proc has temporarily run out of data").
const STARVATION_STATUS: i32 = 1;
/// AAC-LC frames are always 1024 samples per channel — a fixed property of the format, not
/// driver-dependent.
const AAC_FRAME_SAMPLES: u32 = 1024;
/// Output scratch buffer capacity per `AudioConverterFillComplexBuffer` call — generous headroom
/// over a typical AAC-LC frame's real size (a few hundred bytes at common bitrates); rejected as
/// `EncodeError::Backend` if a real frame ever needs more (never silently truncated).
const OUTPUT_BUF_CAP: usize = 8192;
/// Default bitrate when [`AudioEncoderConfig::bitrate_bps`] is `0` — matches
/// `mediaway-encoder::windows::wmf::aac`'s identical default.
const DEFAULT_BITRATE_BPS: u32 = 128_000;

/// AAC-LC encode session over `AudioConverter` (Float32 interleaved PCM input only — see
/// ADR-0004 § Scope).
pub(crate) struct AacEncoder {
    converter: AudioConverterRef,
    info: StreamInfo,
    sample_rate: u32,
    channels: u16,
    time_base: Rational,
    /// Interleaved F32 bytes accumulated from `push_frame`, not yet consumed by the converter.
    /// `read_pos` bytes at the front are already-consumed and periodically compacted away.
    pcm: Vec<u8>,
    read_pos: usize,
    /// Total samples (per channel) the converter has accepted as input so far — the basis for
    /// computing each output packet's `pts` (`AudioConverter` does not return one itself, unlike
    /// `VideoToolbox`'s `CMSampleBuffer`).
    samples_consumed: u64,
    first_pts: Option<i64>,
    extradata_read: bool,
    pending: VecDeque<Packet>,
    flushed: bool,
}

// SAFETY: `AudioConverterRef` is an opaque Core Audio object; every use of `self.converter` goes
// through this type's own `&mut self` API, which already enforces exclusive access on the Rust
// side. Unlike this crate's `VideoToolbox` backends, `AudioConverter` has no asynchronous
// callback running on a separate thread — `AudioConverterFillComplexBuffer` and its input
// callback are both synchronous, nested on the calling thread (see ADR-0004 § Context).
unsafe impl Send for AacEncoder {}

impl AacEncoder {
    /// Open an `AudioConverter` AAC-LC encode session for `config`.
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
        // Compressed-format ASBD: only the fields `AudioConverterNew` needs to pick a codec are
        // set; the rest are resolved internally (see ADR-0004 § Decision — informed by public
        // `AudioConverter`+AAC reference usage, not literally spelled out in the local doc
        // comments).
        let destination = AudioStreamBasicDescription {
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

        let bitrate = if config.bitrate_bps == 0 {
            DEFAULT_BITRATE_BPS
        } else {
            config.bitrate_bps
        };
        // Best-effort — some converters only accept a bitrate from a fixed
        // `kAudioConverterApplicableEncodeBitRates` set and reject an arbitrary value; a
        // rejection is not fatal to opening a working (if default-bitrate) session.
        let mut bitrate_value = bitrate;
        // SAFETY: `converter` is a valid, just-created `AudioConverterRef`;
        // `kAudioConverterEncodeBitRate`'s documented value type is a plain `UInt32`, matching
        // `bitrate_value`'s size passed as `io_property_data_size` below.
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
            pcm: Vec::new(),
            read_pos: 0,
            samples_consumed: 0,
            first_pts: None,
            extradata_read: false,
            pending: VecDeque::new(),
            flushed: false,
        })
    }

    /// Append PCM, then pull as many complete AAC frames as are ready.
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
            // `AudioBufferList` has a private zero-sized marker field (`_this_is_unsized`,
            // upstream's own "do not construct this directly" guard) — build it in place via
            // `MaybeUninit` + raw-pointer field writes instead of a struct literal. Sound: the
            // marker field is `()`, which has exactly one always-valid bit pattern (zero bytes),
            // so `assume_init()` is safe once every field with real size (`mNumberBuffers`,
            // `mBuffers`) has been written.
            let mut output_list = std::mem::MaybeUninit::<AudioBufferList>::uninit();
            let list_ptr = output_list.as_mut_ptr();
            // SAFETY: `list_ptr` is valid, properly aligned pointer for a full
            // `AudioBufferList` (from a stack-local `MaybeUninit`); `addr_of_mut!` never forms
            // a reference to the not-yet-initialized value, only computes field offsets — sound
            // per `MaybeUninit`'s own documented field-initialization pattern.
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
            // same-thread, nested) duration — see this module's own `unsafe impl Send` comment
            // and ADR-0004 § Context for why no `Arc`/refcon lifetime management is needed here,
            // unlike this crate's `VideoToolbox` backends; `output_list`'s one buffer points at
            // `output`, a valid `OUTPUT_BUF_CAP`-byte stack array; `packet_desc` is a valid
            // single-slot out-array (`io_output_packet_size` never exceeds `1`).
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
                // Nothing produced this round — either genuine starvation (not enough buffered
                // PCM for one more 1024-sample frame yet) or a real converter error; either way,
                // stop and wait for more `push_frame` input.
                break;
            }

            // `AudioConverterFillComplexBuffer`'s own doc: "On exit, mDataByteSize is set to the
            // number of bytes written" — `output_list` is the exact memory the C call wrote
            // through (`NonNull::from(&mut output_list)` above), so read the real count back
            // from it, not the pre-call capacity.
            let len = usize::try_from(output_list.mBuffers[0].mDataByteSize).unwrap_or(0);
            if len == 0 || len > OUTPUT_BUF_CAP {
                return Err(EncodeError::Backend);
            }
            let payload = Bytes::copy_from_slice(&output[..len]);

            if !self.extradata_read {
                self.try_read_extradata();
            }

            let pts = self
                .first_pts
                .map_or(0, |first| first + self.output_pts_offset());
            self.pending.push_back(Packet {
                stream_id: 0,
                pts,
                dts: pts,
                duration: duration_ticks(AAC_FRAME_SAMPLES, self.sample_rate, self.time_base),
                is_keyframe: true,
                is_discard: false,
                payload,
            });

            if status != NO_ERROR {
                // Output was produced, but the callback also signaled starvation on its last
                // invocation this round — matches the documented "stop and return what was
                // already produced" contract; nothing more to drain right now.
                break;
            }
        }

        // Compact: drop the already-consumed prefix so `self.pcm` does not grow unboundedly
        // across the life of a long session.
        if self.read_pos > 0 {
            self.pcm.drain(..self.read_pos);
            self.read_pos = 0;
        }
        Ok(())
    }

    /// Samples already emitted as complete output packets (including the one just produced this
    /// round), converted to `time_base` ticks — the offset from `first_pts` for that packet.
    /// Assumes each output packet corresponds to exactly `AAC_FRAME_SAMPLES` newly-consumed
    /// input samples — true for AAC-LC's fixed 1024-sample framing absent encoder look-ahead/
    /// priming delay, which this Stage-1 implementation does not account for (unverifiable
    /// without real hardware — see ADR-0004's zero-compile-verification caveat).
    fn output_pts_offset(&self) -> i64 {
        let ticks = duration_ticks(
            u32::try_from(
                self.samples_consumed
                    .saturating_sub(u64::from(AAC_FRAME_SAMPLES)),
            )
            .unwrap_or(u32::MAX),
            self.sample_rate,
            self.time_base,
        );
        i64::try_from(ticks).unwrap_or(i64::MAX)
    }

    fn try_read_extradata(&mut self) {
        let mut size = u32::try_from(64usize).unwrap_or(0);
        let mut buf = [0u8; 64];
        // SAFETY: `self.converter` is a valid, open `AudioConverterRef`; `buf` is a valid
        // 64-byte stack array (`AudioSpecificConfig` for AAC-LC is 2-5 bytes, far under this
        // capacity); a failure here just means the cookie is not ready yet — harmless, retried
        // on the next output packet via `self.extradata_read` staying `false`.
        // SAFETY (continued): `size` starts at `buf`'s real capacity per
        // `AudioConverterGetProperty`'s "on entry, the size of the memory pointed to by
        // outPropertyData" contract; `NonNull::from(&mut buf)` is a valid pointer for that many
        // bytes.
        let status = unsafe {
            AudioConverterGetProperty(
                self.converter,
                kAudioConverterCompressionMagicCookie,
                NonNull::from(&mut size),
                NonNull::from(&mut buf).cast(),
            )
        };
        let len = usize::try_from(size).unwrap_or(0);
        if status != NO_ERROR || len == 0 || len > buf.len() {
            return;
        }
        if let StreamInfo::Audio { extra_data, .. } = &mut self.info {
            *extra_data = Bytes::copy_from_slice(&buf[..len]);
        }
        self.extradata_read = true;
    }
}

impl AudioEncoder for AacEncoder {
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
            // `S16`/`S32` conversion is deferred — see ADR-0004 § Scope.
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
        // Any PCM buffered but short of one more full 1024-sample AAC-LC frame is not encoded —
        // a documented, deliberate scope cut (see ADR-0004 § Scope), not silently padded/guessed.
        Ok(())
    }
}

impl Drop for AacEncoder {
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

/// `AudioConverterComplexInputDataProc` — synchronous, nested inside the still-executing
/// `AudioConverterFillComplexBuffer` call that invoked it (see ADR-0004 § Context; no
/// cross-thread concern, unlike this crate's `VideoToolbox` callbacks).
///
/// # Safety
///
/// `in_user_data` must be exactly the `&mut InputContext` pointer
/// [`AacEncoder::drain_ready_packets`] passed as `in_input_data_proc_user_data` for the one
/// `AudioConverterFillComplexBuffer` call this callback is nested inside.
unsafe extern "C-unwind" fn input_proc(
    _in_audio_converter: AudioConverterRef,
    io_number_data_packets: NonNull<u32>,
    io_data: NonNull<AudioBufferList>,
    _out_data_packet_description: *mut *mut AudioStreamPacketDescription,
    in_user_data: *mut c_void,
) -> i32 {
    // SAFETY: per this function's own safety contract above.
    let ctx = unsafe { &mut *(in_user_data.cast::<InputContext>()) };
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
        mNumberChannels: ctx.channels, // interleaved: one buffer carries all channels
        mDataByteSize: u32::try_from(slice.len()).unwrap_or(0),
        // SAFETY: `slice` borrows `ctx.pcm`, itself borrowed from `AacEncoder::pcm` for the
        // whole duration of the enclosing `AudioConverterFillComplexBuffer` call — per that
        // API's own documented contract ("the callback is responsible for not freeing or
        // altering this buffer until it is called again"), `AudioConverter` only reads from a
        // caller-supplied input buffer, never writes to it, despite the C signature's `*mut`
        // (a common C-API constness gap, not a real mutation).
        mData: slice.as_ptr().cast_mut().cast::<c_void>(),
    };
    // SAFETY: `io_data` is a valid, callback-scoped out-pointer; writing one `AudioBuffer` into
    // its single `mBuffers` slot matches this backend's `mNumberBuffers = 1` interleaved-PCM
    // shape (the only input shape `AacEncoder` supports).
    unsafe {
        (*io_data.as_ptr()).mNumberBuffers = 1;
        (*io_data.as_ptr()).mBuffers[0] = buffer;
        *io_number_data_packets.as_ptr() = give_frames;
    }
    ctx.consumed += give_bytes;
    NO_ERROR
}

/// Ticks (in `time_base` units) for `samples` frames at `sample_rate` — `i128` intermediate to
/// avoid overflow, mirrors this crate's `VideoToolbox` `CMTime` tick-math convention.
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
    if config.codec != CodecKind::Aac {
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
        codec: CodecKind::Aac,
        time_base: config.time_base,
        extra_data: Bytes::new(),
        sample_rate: config.sample_rate,
        channels: config.channels,
    }
}
