//! Auto audio encode C ABI — `mediaway_encoder::AudioEncoder` reachable from C.
//!
//! Design: `adr/0003-auto-audio-encode-c-abi.md` — single-step open (the encode
//! session *is* the encoder; no intermediate handle, so no consumption trap),
//! borrowed PCM input views, owned packet/stream-info outputs with dedicated
//! frees, `catch_unwind` panic safety, and a hand-written header
//! (`include/mediaway/pipeline.h`, ABI v2).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::slice;

use mediaway_common::{AudioFrame, Bytes, CodecKind, SampleFormat, StreamInfo};
use mediaway_encoder::{AudioEncoder, AudioEncoderConfig, EncodeError};

use crate::buffer::{leak_boxed_slice, reclaim_boxed_slice};
use crate::status::MediawayPipelineStatus;
use crate::types::{
    MediawayAudioEncodeConfig, MediawayAudioFrameView, MediawayAudioPacket,
    MediawayAudioStreamInfo, MediawayPipelineCodecKind, MediawayRational, MediawaySampleFormat,
};

/// The encode session handle — `Box<dyn AudioEncoder>`, the same thin-pointer
/// pattern as `AutoEncoderHandle` (`adr/0001` §3).
///
/// Needs no `poisoned` flag: `close` is always safe, and every other function's
/// failure path returns a status without destroying the handle (the caller
/// chooses to close).
pub type AudioEncodeSessionHandle = Box<dyn AudioEncoder>;

/// Validate + translate a C config into the Rust `AudioEncoderConfig`.
fn rust_config(
    config: &MediawayAudioEncodeConfig,
) -> Result<AudioEncoderConfig, MediawayPipelineStatus> {
    let codec = match config.codec {
        MediawayPipelineCodecKind::Aac => CodecKind::Aac,
        _ => return Err(MediawayPipelineStatus::Unsupported), // only AAC today
    };
    let sample_format = match config.sample_format {
        MediawaySampleFormat::F32 => SampleFormat::F32,
        _ => return Err(MediawayPipelineStatus::Unsupported), // only F32 accepted today
    };
    if config.sample_rate == 0 || config.channels == 0 {
        return Err(MediawayPipelineStatus::InvalidInput);
    }
    Ok(AudioEncoderConfig {
        codec,
        sample_rate: config.sample_rate,
        channels: config.channels,
        sample_format,
        time_base: config.time_base.into(),
        bitrate_bps: config.bitrate_bps,
    })
}

/// Open the best available audio encoder on the current platform. Mirrors
/// `mediaway_pipeline::platform::AutoEncoder`'s dispatch shape: Windows reaches
/// the real WMF AAC backend; every other platform returns
/// `EncodeError::NoBackend` (graceful, not an error the caller must treat as a
/// bug).
fn open_audio_encoder(config: &AudioEncoderConfig) -> Result<Box<dyn AudioEncoder>, EncodeError> {
    #[cfg(windows)]
    {
        Ok(Box::new(
            mediaway_encoder_windows::WindowsAudioEncoder::open(config)?,
        ))
    }
    #[cfg(not(windows))]
    {
        let _ = config;
        Err(EncodeError::NoBackend)
    }
}

/// Open the best available audio encoder for `config`.
///
/// The returned handle **is** the encode session (`adr/0003` § Decision: audio
/// has no internal muxer, so the video surface's two-step open has nothing to
/// add here).
///
/// Three outcomes: (1) `Ok` — builds the handle, writes it to `*out_session`;
/// (2) a normal `Err` (e.g. `NO_BACKEND` on non-Windows, `UNSUPPORTED` for a
/// non-AAC/non-F32 config) — no handle exists, `*out_session` is set to `NULL`,
/// the matching status is returned; (3) a caught panic — same
/// `NULL`/`INTERNAL_PANIC` shape as (2).
///
/// # Safety
///
/// `config` must be a valid, readable [`MediawayAudioEncodeConfig`] pointer.
/// `out_session` must be a valid, writable, non-null out-parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_audio_encoder_open(
    config: *const MediawayAudioEncodeConfig,
    out_session: *mut *mut AudioEncodeSessionHandle,
) -> MediawayPipelineStatus {
    if config.is_null() || out_session.is_null() {
        return MediawayPipelineStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `config` is valid for reads (function contract).
    let config = unsafe { *config };
    // SAFETY: `out_session` is checked non-null above (function contract).
    unsafe { out_session.write(ptr::null_mut()) };

    let result = catch_unwind(AssertUnwindSafe(|| {
        let rust = rust_config(&config)?;
        open_audio_encoder(&rust).map_err(MediawayPipelineStatus::from)
    }));

    match result {
        Ok(Ok(encoder)) => {
            let handle: Box<AudioEncodeSessionHandle> = Box::new(encoder);
            // SAFETY: `out_session` is checked non-null above (function contract).
            unsafe { out_session.write(Box::into_raw(handle)) };
            MediawayPipelineStatus::Ok
        }
        Ok(Err(err)) => err,
        Err(_) => MediawayPipelineStatus::InternalPanic,
    }
}

/// Query the session's stream metadata (codec, timebase, rates, codec config).
///
/// `extra_data` is the `AudioSpecificConfig` a muxer's audio track needs to be
/// playable. Owned output; release with
/// [`mediaway_pipeline_ffi_stream_info_free`].
///
/// # Safety
///
/// `session` must be a valid handle pointer; `out_info` a valid, writable,
/// non-null out-parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_audio_encode_session_stream_info(
    session: *const AudioEncodeSessionHandle,
    out_info: *mut MediawayAudioStreamInfo,
) -> MediawayPipelineStatus {
    if session.is_null() || out_info.is_null() {
        return MediawayPipelineStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `out_info` is writable (function contract).
    unsafe { ptr::write(out_info, MediawayAudioStreamInfo::default()) };

    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `session` is a valid handle pointer (function contract).
        let encoder = unsafe { &**session };
        match encoder.stream_info() {
            StreamInfo::Audio {
                codec,
                time_base,
                extra_data,
                sample_rate,
                channels,
                ..
            } => {
                // `Bytes` is immutable/refcounted and the FFI boundary hands
                // the caller a raw pointer it alone frees — copy into a
                // boxed slice via the shared leak helper (released with
                // mediaway_pipeline_ffi_stream_info_free).
                let (extra_ptr, extra_len) = leak_boxed_slice(extra_data.to_vec());
                Ok(MediawayAudioStreamInfo {
                    codec: (*codec).into(),
                    time_base: (*time_base).into(),
                    sample_rate: *sample_rate,
                    channels: *channels,
                    extra_data: extra_ptr,
                    extra_data_len: extra_len,
                })
            }
            _ => Err(MediawayPipelineStatus::UnknownError),
        }
    }));

    match result {
        Ok(Ok(info)) => {
            // SAFETY: `out_info` is checked non-null above (function contract).
            unsafe { out_info.write(info) };
            MediawayPipelineStatus::Ok
        }
        Ok(Err(status)) => status,
        Err(_) => MediawayPipelineStatus::InternalPanic,
    }
}

/// Push one PCM buffer into the encoder.
///
/// `frame` is a **borrowed** view, valid for the duration of this call only —
/// the encoder copies synchronously (adr/0003 § Known cost: same class as the
/// video CPU-upload path).
///
/// # Safety
///
/// `session` must be a valid handle pointer; `frame` a valid, readable pointer
/// whose `data` (when `data_len > 0`) points to `data_len` readable bytes,
/// both valid for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_audio_encode_session_push_pcm(
    session: *mut AudioEncodeSessionHandle,
    frame: *const MediawayAudioFrameView,
) -> MediawayPipelineStatus {
    if session.is_null() || frame.is_null() {
        return MediawayPipelineStatus::InvalidArgument;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `frame` is valid for reads (function contract).
        let view = unsafe { &*frame };
        let format = match view.sample_format {
            MediawaySampleFormat::F32 => SampleFormat::F32,
            _ => return Err(MediawayPipelineStatus::Unsupported),
        };
        if view.data.is_null() && view.data_len > 0 {
            return Err(MediawayPipelineStatus::InvalidArgument);
        }
        let data = if view.data_len == 0 {
            Bytes::new()
        } else {
            // SAFETY: caller guarantees `data` points to `data_len` readable
            // bytes for the duration of the call; we copy synchronously.
            unsafe { Bytes::copy_from_slice(slice::from_raw_parts(view.data, view.data_len)) }
        };
        let rust = AudioFrame {
            pts: view.pts,
            duration: view.duration,
            sample_rate: view.sample_rate,
            channels: view.channels,
            format,
            data,
        };
        // SAFETY: caller guarantees `session` is a valid handle pointer (function contract).
        let handle = unsafe { &mut *session };
        handle
            .push_frame(&rust)
            .map_err(MediawayPipelineStatus::from)
    }));
    match result {
        Ok(Ok(())) => MediawayPipelineStatus::Ok,
        Ok(Err(status)) => status,
        Err(_) => MediawayPipelineStatus::InternalPanic,
    }
}

/// Pull the next encoded packet, if any is ready.
///
/// When `*out_has_packet` is written `true`, `*out_packet` is an OWNED output
/// — release it with [`mediaway_pipeline_ffi_packet_free`]. `false` is a valid
/// "nothing ready" result, not an error.
///
/// # Safety
///
/// `session` must be a valid handle pointer; `out_packet`/`out_has_packet`
/// valid, writable, non-null out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_audio_encode_session_poll_packet(
    session: *mut AudioEncodeSessionHandle,
    out_packet: *mut MediawayAudioPacket,
    out_has_packet: *mut bool,
) -> MediawayPipelineStatus {
    if session.is_null() || out_packet.is_null() || out_has_packet.is_null() {
        return MediawayPipelineStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `out_packet` is writable (function contract).
    unsafe { ptr::write(out_packet, MediawayAudioPacket::default()) };
    // SAFETY: caller guarantees `out_has_packet` is writable (function contract).
    unsafe { ptr::write(out_has_packet, false) };

    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `session` is a valid handle pointer (function contract).
        let handle = unsafe { &mut *session };
        handle.poll_packet().map_err(MediawayPipelineStatus::from)
    }));

    match result {
        Ok(Ok(Some(packet))) => {
            // `Bytes` is immutable/refcounted and the C side frees the payload
            // via the shared reclaim helper — copy into a boxed slice we hand
            // over.
            let (payload_ptr, payload_len) = leak_boxed_slice(packet.payload.to_vec());
            // SAFETY: `out_packet` is checked non-null above (function contract).
            unsafe {
                out_packet.write(MediawayAudioPacket {
                    pts: packet.pts,
                    dts: packet.dts,
                    duration: packet.duration,
                    is_keyframe: packet.is_keyframe,
                    is_discard: packet.is_discard,
                    payload: payload_ptr,
                    payload_len,
                });
            };
            // SAFETY: `out_has_packet` is checked non-null above (function contract).
            unsafe { ptr::write(out_has_packet, true) };
            MediawayPipelineStatus::Ok
        }
        Ok(Ok(None)) => MediawayPipelineStatus::Ok,
        Ok(Err(status)) => status,
        Err(_) => MediawayPipelineStatus::InternalPanic,
    }
}

/// Signal end-of-input.
///
/// Drain the remaining packets with
/// [`mediaway_audio_encode_session_poll_packet`] afterwards.
///
/// # Safety
///
/// `session` must be a valid handle pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_audio_encode_session_flush(
    session: *mut AudioEncodeSessionHandle,
) -> MediawayPipelineStatus {
    if session.is_null() {
        return MediawayPipelineStatus::InvalidArgument;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `session` is a valid handle pointer (function contract).
        let handle = unsafe { &mut *session };
        handle.flush().map_err(MediawayPipelineStatus::from)
    }));
    match result {
        Ok(Ok(())) => MediawayPipelineStatus::Ok,
        Ok(Err(status)) => status,
        Err(_) => MediawayPipelineStatus::InternalPanic,
    }
}

/// Close and free an audio encode session. Always safe to call, including on a
/// null pointer (a no-op) — this surface has no handle-consumption trap
/// (`adr/0003` § Decision).
///
/// # Safety
///
/// `session` must be null or a pointer previously returned by
/// [`mediaway_audio_encoder_open`] and not already closed by this function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_audio_encode_session_close(
    session: *mut AudioEncodeSessionHandle,
) {
    if session.is_null() {
        return;
    }
    // A panic during drop is deliberately swallowed and the allocation leaked —
    // same reasoning as `mediaway_auto_encoder_close` (`adr/0001` §7).
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `session` is a valid, not-yet-closed handle
        // pointer (function contract).
        drop(unsafe { Box::from_raw(session) });
    }));
}

/// Free a packet returned by [`mediaway_audio_encode_session_poll_packet`].
/// Nulls `payload`/`payload_len` afterward, making a double-free a visible
/// no-op instead of undefined behavior.
///
/// # Safety
///
/// `packet` must be null or a pointer previously filled by
/// [`mediaway_audio_encode_session_poll_packet`] and not already freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_pipeline_ffi_packet_free(packet: *mut MediawayAudioPacket) {
    if packet.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `packet` is a valid, not-yet-freed pointer
        // (function contract).
        let packet = unsafe { &mut *packet };
        // SAFETY: `payload`/`payload_len` were produced by `leak_boxed_slice`
        // in poll_packet (function contract).
        unsafe { reclaim_boxed_slice(packet.payload, packet.payload_len) };
        packet.payload = ptr::null_mut();
        packet.payload_len = 0;
    }));
}

/// Free stream info returned by
/// [`mediaway_audio_encode_session_stream_info`]. Nulls `extra_data`/
/// `extra_data_len` afterward, making a double-free a visible no-op.
///
/// # Safety
///
/// `info` must be null or a pointer previously filled by
/// [`mediaway_audio_encode_session_stream_info`] and not already freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_pipeline_ffi_stream_info_free(
    info: *mut MediawayAudioStreamInfo,
) {
    if info.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `info` is a valid, not-yet-freed pointer
        // (function contract).
        let info = unsafe { &mut *info };
        // SAFETY: `extra_data`/`extra_data_len` were produced by
        // `leak_boxed_slice` in stream_info (function contract).
        unsafe { reclaim_boxed_slice(info.extra_data, info.extra_data_len) };
        info.extra_data = ptr::null_mut();
        info.extra_data_len = 0;
    }));
}

// Defaults for the plain-value out structs above — zeroed, matching the
// "kind/flag decides which fields matter" convention of the other headers.
impl Default for MediawayAudioStreamInfo {
    fn default() -> Self {
        Self {
            codec: MediawayPipelineCodecKind::Aac,
            time_base: MediawayRational { num: 0, den: 0 },
            sample_rate: 0,
            channels: 0,
            extra_data: ptr::null_mut(),
            extra_data_len: 0,
        }
    }
}

impl Default for MediawayAudioPacket {
    fn default() -> Self {
        Self {
            pts: 0,
            dts: 0,
            duration: 0,
            is_keyframe: false,
            is_discard: false,
            payload: ptr::null_mut(),
            payload_len: 0,
        }
    }
}
