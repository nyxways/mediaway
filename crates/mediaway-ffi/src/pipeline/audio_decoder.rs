//! Audio decode session C ABI — Opus (software) and AAC (the OS's decoder).
//!
//! Design: `adr/pipeline/0006-audio-decode-c-abi.md` (Opus) and
//! `adr/pipeline/0007-stream-bytes-aac-decode-support-probe.md` §2 (AAC) — single-step open
//! (the handle *is* the decoder, same shape as [`crate::pipeline::decoder`]'s video surface),
//! `poisoned`-guarded (`push_packet`/`poll_frame` are repeated-call APIs).
//!
//! The session dispatches over a closed `enum` of exactly the decoders this entry point is
//! defined as, not a `Box<dyn AudioDecoder>` (`docs/spec/zero-cost-abstractions.md`).
//! **Opus** is `mediaway_sw::opus::OpusDecoder` on every platform, so a C caller gets
//! byte-identical output with no OS codec dependency. **AAC** is `windows::WmfAacDecoder` or
//! `apple::AacDecoder`: the OS's own codec, so its samples can differ between hosts, and it is
//! `UNSUPPORTED` elsewhere. That asymmetry is why `mediaway_decoder_support` exists.
//!
//! An empty `payload` in [`mediaway_audio_decode_session_push_packet`] is Opus's
//! packet-loss-concealment hint, passed straight through — the same contract
//! `mediaway_sw::opus::OpusDecoder::push_packet` documents. AAC has no such convention, so
//! an empty AAC packet is `INVALID_INPUT`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mediaway_common::{AudioFrame, Bytes, Packet};
#[cfg(any(windows, target_os = "macos", target_os = "ios"))]
use mediaway_decoder::AudioDecoder;
use mediaway_sw::opus::config::OpusDecoderConfig;
use mediaway_sw::opus::decoder::OpusDecoder;

use crate::pipeline::buffer::{borrow_slice, leak_boxed_slice, reclaim_boxed_slice};
use crate::pipeline::status::MediawayPipelineStatus;
use crate::pipeline::types::{
    MediawayAudioDecodeConfig, MediawayDecodePacketView, MediawayDecodedAudioFrame,
    MediawayPipelineCodecKind,
};

/// Opaque audio decode-session handle (`mediaway_audio_decode_session_t*` in the C
/// header). See module docs for why this wraps `OpusDecoder` directly instead of a
/// trait object.
///
/// Thread-confined by convention, same as every other handle in this crate: may be
/// moved between threads, but must not be used from two threads concurrently
/// without external synchronization.
pub struct AudioDecodeSessionHandle {
    poisoned: bool,
    inner: AudioDecoderInner,
}

/// The OS's own AAC decoder on this platform (`adr/pipeline/0007` §2). Only Windows and Apple
/// have one; everywhere else `AAC` is `UNSUPPORTED` and this type does not exist.
#[cfg(windows)]
type PlatformAacDecoder = mediaway_decoder::windows::WmfAacDecoder;
#[cfg(any(target_os = "macos", target_os = "ios"))]
type PlatformAacDecoder = mediaway_decoder::apple::AacDecoder;

/// What a session decodes with. Enum dispatch over a set that is closed at compile time, not a
/// `Box<dyn AudioDecoder>` (`docs/spec/zero-cost-abstractions.md`): the variants are the two
/// decoders this entry point is defined as.
enum AudioDecoderInner {
    Opus(OpusDecoder),
    #[cfg(any(windows, target_os = "macos", target_os = "ios"))]
    Aac(PlatformAacDecoder),
}

impl AudioDecoderInner {
    fn push_packet(&mut self, packet: &Packet) -> Result<(), MediawayPipelineStatus> {
        match self {
            Self::Opus(d) => d.push_packet(packet).map_err(Into::into),
            #[cfg(any(windows, target_os = "macos", target_os = "ios"))]
            Self::Aac(d) => {
                // Opus's empty packet means "lost frame, conceal it". AAC has no such
                // convention, so an empty one is a caller mistake, not a hint.
                if packet.payload.is_empty() {
                    return Err(MediawayPipelineStatus::InvalidInput);
                }
                AudioDecoder::push_packet(d, packet).map_err(Into::into)
            }
        }
    }

    fn poll_frame(&mut self) -> Result<Option<AudioFrame>, MediawayPipelineStatus> {
        match self {
            Self::Opus(d) => d.poll_frame().map_err(Into::into),
            #[cfg(any(windows, target_os = "macos", target_os = "ios"))]
            Self::Aac(d) => AudioDecoder::poll_frame(d).map_err(Into::into),
        }
    }

    fn flush(&mut self) -> Result<(), MediawayPipelineStatus> {
        match self {
            Self::Opus(d) => d.flush().map_err(Into::into),
            #[cfg(any(windows, target_os = "macos", target_os = "ios"))]
            Self::Aac(d) => AudioDecoder::flush(d).map_err(Into::into),
        }
    }
}

/// Open the AAC decoder for `config`. The raw `AudioSpecificConfig` is required: both backends
/// refuse to synthesise one, because a default would decode SBR/PS streams to quietly wrong
/// output, and an empty one is a config mistake rather than a missing capability.
#[cfg(any(windows, target_os = "macos", target_os = "ios"))]
fn open_aac(
    config: &MediawayAudioDecodeConfig,
) -> Result<AudioDecoderInner, MediawayPipelineStatus> {
    // SAFETY: `config.extra_data`/`extra_data_len` describe a buffer valid for the
    // `mediaway_audio_decode_session_open` call (function contract).
    let Some(asc) = (unsafe { borrow_slice(config.extra_data, config.extra_data_len) }) else {
        return Err(MediawayPipelineStatus::InvalidArgument);
    };
    if asc.is_empty() {
        return Err(MediawayPipelineStatus::InvalidInput);
    }
    let extra_data = Bytes::copy_from_slice(asc);
    let time_base = config.time_base.into();

    #[cfg(windows)]
    let decoder = {
        use mediaway_decoder::windows::AacDecoderConfig;
        let mut cfg = AacDecoderConfig::new(config.sample_rate, config.channels, extra_data);
        cfg.time_base = time_base;
        PlatformAacDecoder::open(&cfg)
    };
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let decoder = {
        use mediaway_decoder::apple::AacDecoderConfig;
        let cfg = AacDecoderConfig::new(config.sample_rate, config.channels, time_base, extra_data);
        PlatformAacDecoder::open(&cfg)
    };
    decoder.map(AudioDecoderInner::Aac).map_err(Into::into)
}

/// No OS AAC decoder on this platform.
#[cfg(not(any(windows, target_os = "macos", target_os = "ios")))]
const fn open_aac(
    _: &MediawayAudioDecodeConfig,
) -> Result<AudioDecoderInner, MediawayPipelineStatus> {
    Err(MediawayPipelineStatus::Unsupported)
}

fn open_decoder(
    config: &MediawayAudioDecodeConfig,
) -> Result<AudioDecoderInner, MediawayPipelineStatus> {
    match config.codec {
        MediawayPipelineCodecKind::Opus => {
            let sw_config = OpusDecoderConfig::new(
                config.sample_rate,
                config.channels,
                config.time_base.into(),
            );
            OpusDecoder::open(&sw_config)
                .map(AudioDecoderInner::Opus)
                .map_err(Into::into)
        }
        MediawayPipelineCodecKind::Aac => open_aac(config),
        _ => Err(MediawayPipelineStatus::Unsupported),
    }
}

/// Open an Opus decode session for `config`.
///
/// Three outcomes: (1) `Ok` — builds the handle, writes it to `*out_session`; (2) a
/// normal `Err` (e.g. `config.codec != Opus` → `Unsupported`) — no handle exists,
/// `*out_session` is set to `NULL`, the matching status is returned; (3) a caught
/// panic — same `NULL`/[`MediawayPipelineStatus::InternalPanic`] shape as (2).
///
/// # Safety
///
/// `config` must be a valid, readable [`MediawayAudioDecodeConfig`] pointer.
/// `out_session` must be a valid, writable, non-null out-parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_audio_decode_session_open(
    config: *const MediawayAudioDecodeConfig,
    out_session: *mut *mut AudioDecodeSessionHandle,
) -> MediawayPipelineStatus {
    if config.is_null() || out_session.is_null() {
        return MediawayPipelineStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `config` is valid for reads (function contract).
    let config = unsafe { *config };
    // SAFETY: `out_session` is checked non-null above; caller guarantees it is
    // writable (function contract).
    unsafe { out_session.write(std::ptr::null_mut()) };

    if !matches!(
        config.codec,
        MediawayPipelineCodecKind::Opus | MediawayPipelineCodecKind::Aac
    ) {
        return MediawayPipelineStatus::Unsupported; // Opus and AAC only
    }
    if config.sample_rate == 0 || config.channels == 0 {
        return MediawayPipelineStatus::InvalidInput;
    }

    let result = catch_unwind(AssertUnwindSafe(|| open_decoder(&config)));

    match result {
        Ok(Ok(decoder)) => {
            let handle = Box::new(AudioDecodeSessionHandle {
                poisoned: false,
                inner: decoder,
            });
            // SAFETY: `out_session` is checked non-null above (function contract).
            unsafe { out_session.write(Box::into_raw(handle)) };
            MediawayPipelineStatus::Ok
        }
        Ok(Err(status)) => status,
        Err(_) => MediawayPipelineStatus::InternalPanic,
    }
}

/// Push one compressed Opus packet. May produce zero or more frames (drain via
/// [`mediaway_audio_decode_session_poll_frame`]).
///
/// `packet`'s `payload` is a caller-owned borrow, valid for the call only — the
/// core copies it synchronously. An empty payload (`payload == NULL` or
/// `payload_len == 0`) is Opus's packet-loss-concealment hint for a lost frame, not
/// an error.
///
/// # Safety
///
/// `session` must be a valid, live handle pointer. `packet` must be a valid,
/// readable pointer whose `payload` (when `payload_len > 0`) points to
/// `payload_len` readable bytes, both valid for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_audio_decode_session_push_packet(
    session: *mut AudioDecodeSessionHandle,
    packet: *const MediawayDecodePacketView,
) -> MediawayPipelineStatus {
    if session.is_null() || packet.is_null() {
        return MediawayPipelineStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `session` is a valid, live handle pointer (function
    // contract).
    let handle = unsafe { &mut *session };
    if handle.poisoned {
        return MediawayPipelineStatus::HandlePoisoned;
    }
    // SAFETY: caller guarantees `packet` is valid for reads (function contract).
    let view = unsafe { *packet };
    // SAFETY: `view.payload`/`view.payload_len` describe a buffer valid for this call
    // (function contract). `payload == NULL && payload_len == 0` is a legal Opus
    // packet-loss-concealment hint — `borrow_slice` already returns `Some(&[])` for
    // that exact pair, not `None` (only a mismatched null-with-nonzero-length pair
    // is rejected below).
    let Some(payload) = (unsafe { borrow_slice(view.payload, view.payload_len) }) else {
        return MediawayPipelineStatus::InvalidArgument;
    };

    let result = catch_unwind(AssertUnwindSafe(|| {
        let packet = Packet {
            stream_id: view.stream_id,
            pts: view.pts,
            dts: view.dts,
            duration: view.duration,
            is_keyframe: view.is_keyframe,
            is_discard: view.is_discard,
            payload: Bytes::copy_from_slice(payload),
        };
        handle.inner.push_packet(&packet)
    }));

    match result {
        Ok(Ok(())) => MediawayPipelineStatus::Ok,
        Ok(Err(status)) => status,
        Err(_) => {
            handle.poisoned = true;
            MediawayPipelineStatus::InternalPanic
        }
    }
}

/// Pull the next decoded PCM frame, if any is ready.
///
/// `*out_has_frame == false` is a valid "nothing ready" result, not an error. When
/// `true`, release `*out_frame` with [`mediaway_decoded_audio_frame_free`].
///
/// # Safety
///
/// `session` must be a valid, live handle pointer. `out_frame`/`out_has_frame` must
/// be valid, writable, non-null out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_audio_decode_session_poll_frame(
    session: *mut AudioDecodeSessionHandle,
    out_frame: *mut MediawayDecodedAudioFrame,
    out_has_frame: *mut bool,
) -> MediawayPipelineStatus {
    if session.is_null() || out_frame.is_null() || out_has_frame.is_null() {
        return MediawayPipelineStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `session` is a valid, live handle pointer (function
    // contract).
    let handle = unsafe { &mut *session };
    if handle.poisoned {
        return MediawayPipelineStatus::HandlePoisoned;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        let maybe_frame = handle.inner.poll_frame()?;
        let Some(frame) = maybe_frame else {
            return Ok(None);
        };
        let (data_ptr, data_len) = leak_boxed_slice(frame.data.to_vec());
        Ok(Some(MediawayDecodedAudioFrame {
            pts: frame.pts,
            duration: frame.duration,
            sample_rate: frame.sample_rate,
            channels: frame.channels,
            sample_format: frame.format.into(),
            data: data_ptr,
            data_len,
        }))
    }));

    match result {
        Ok(Ok(Some(frame))) => {
            // SAFETY: `out_frame`/`out_has_frame` are checked non-null above
            // (function contract).
            unsafe {
                out_frame.write(frame);
                out_has_frame.write(true);
            }
            MediawayPipelineStatus::Ok
        }
        Ok(Ok(None)) => {
            // SAFETY: `out_has_frame` is checked non-null above (function contract).
            unsafe { out_has_frame.write(false) };
            MediawayPipelineStatus::Ok
        }
        Ok(Err(status)) => status,
        Err(_) => {
            handle.poisoned = true;
            MediawayPipelineStatus::InternalPanic
        }
    }
}

/// Signal end-of-input.
///
/// `push_packet` always decodes and enqueues synchronously
/// (`mediaway_sw::opus::OpusDecoder::flush`'s own doc), so this only marks the
/// session closed — call [`mediaway_audio_decode_session_poll_frame`] beforehand to
/// drain any pending frame.
///
/// # Safety
///
/// `session` must be a valid, live handle pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_audio_decode_session_flush(
    session: *mut AudioDecodeSessionHandle,
) -> MediawayPipelineStatus {
    if session.is_null() {
        return MediawayPipelineStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `session` is a valid, live handle pointer (function
    // contract).
    let handle = unsafe { &mut *session };
    if handle.poisoned {
        return MediawayPipelineStatus::HandlePoisoned;
    }

    let result = catch_unwind(AssertUnwindSafe(|| handle.inner.flush()));

    match result {
        Ok(Ok(())) => MediawayPipelineStatus::Ok,
        Ok(Err(status)) => status,
        Err(_) => {
            handle.poisoned = true;
            MediawayPipelineStatus::InternalPanic
        }
    }
}

/// Close and free an audio decode-session handle. Always safe to call, including on
/// a poisoned handle or with `session == NULL`.
///
/// # Safety
///
/// `session` must be null or a pointer previously returned by
/// [`mediaway_audio_decode_session_open`] and not already closed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_audio_decode_session_close(
    session: *mut AudioDecodeSessionHandle,
) {
    if session.is_null() {
        return;
    }
    // A panic during drop is deliberately swallowed and the allocation leaked — same
    // reasoning as `mediaway_decode_session_close`.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `session` is a valid, not-yet-closed handle
        // pointer (function contract).
        drop(unsafe { Box::from_raw(session) });
    }));
}

/// Free a frame returned by [`mediaway_audio_decode_session_poll_frame`]. Nulls
/// `data`/`data_len` afterward, making a double-free a visible no-op. Always safe
/// to call, including with `frame == NULL`.
///
/// # Safety
///
/// `frame` must be null or a valid, writable pointer to a frame previously written
/// by `mediaway_audio_decode_session_poll_frame`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_decoded_audio_frame_free(frame: *mut MediawayDecodedAudioFrame) {
    if frame.is_null() {
        return;
    }
    // SAFETY: caller guarantees `frame` is a valid, writable pointer (function
    // contract).
    let frame = unsafe { &mut *frame };
    // SAFETY: `frame.data`/`frame.data_len` were produced by `leak_boxed_slice` via
    // `mediaway_audio_decode_session_poll_frame` (function contract).
    unsafe { reclaim_boxed_slice(frame.data, frame.data_len) };
    frame.data = std::ptr::null_mut();
    frame.data_len = 0;
}
