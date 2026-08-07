//! Opaque FLV muxer handle and its C ABI functions.
//!
//! Dedicated shape, not a `MuxerState` variant: `flv::Muxer` writes tag bytes directly into
//! a caller-supplied output buffer on every call (`write_header`/`push_packet`) instead of
//! buffering internally for a separate `poll_bytes` step, and has a fixed one-video/
//! one-audio track slot instead of `add_track`'s caller-assigned ids — see
//! `adr/0005-flv-c-abi.md`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mediaway_common::{Bytes, Packet, StreamInfo, VideoGeometry};
use mediaway_container::flv;

use crate::container::buffer::{borrow_slice, leak_boxed_slice};
use crate::container::status::MediawayStatus;
use crate::container::types::{MediawayAudioTrackInfo, MediawayPacketView, MediawayVideoTrackInfo};

/// Opaque FLV muxer handle (`mediaway_flv_muxer_t*` in the C header).
///
/// Thread-confined by convention: may be moved between threads, but must not be used from
/// two threads concurrently without external synchronization.
pub struct FlvMuxerHandle {
    poisoned: bool,
    inner: flv::Muxer,
}

/// Create a new FLV mux session. Call [`mediaway_flv_muxer_write_header`] before any tag.
///
/// Returns null only if a panic was caught during construction (defensive; `flv::Muxer::new`
/// is simple enough that this should not trigger in practice).
#[unsafe(no_mangle)]
pub extern "C" fn mediaway_flv_muxer_create() -> *mut FlvMuxerHandle {
    let built = catch_unwind(AssertUnwindSafe(|| FlvMuxerHandle {
        poisoned: false,
        inner: flv::Muxer::new(),
    }));
    built.map_or(std::ptr::null_mut(), |handle| {
        Box::into_raw(Box::new(handle))
    })
}

/// Write the FLV file header, declaring whether audio/video tags follow.
///
/// Unlike `mediaway_muxer_poll_bytes`, this writes its output directly rather than
/// buffering internally — the returned buffer holds exactly the header bytes from this
/// call. `*out_data == NULL && *out_len == 0` never happens here (the header is always
/// non-empty); the buffer must be released with [`crate::container::mediaway_buffer_free`].
///
/// # Safety
///
/// `muxer` must be a live pointer returned by [`mediaway_flv_muxer_create`]. `out_data` and
/// `out_len` must be valid, writable, non-null out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_flv_muxer_write_header(
    muxer: *mut FlvMuxerHandle,
    has_audio: bool,
    has_video: bool,
    out_data: *mut *mut u8,
    out_len: *mut usize,
) -> MediawayStatus {
    if muxer.is_null() || out_data.is_null() || out_len.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `muxer` is a valid, live handle pointer (function contract).
    let handle = unsafe { &mut *muxer };
    if handle.poisoned {
        return MediawayStatus::HandlePoisoned;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut buf = Vec::new();
        handle.inner.write_header(has_audio, has_video, &mut buf);
        buf
    }));

    if let Ok(buf) = result {
        let (ptr, len) = leak_boxed_slice(buf);
        // SAFETY: `out_data`/`out_len` are checked non-null above (function contract).
        unsafe {
            out_data.write(ptr);
            out_len.write(len);
        }
        MediawayStatus::Ok
    } else {
        handle.poisoned = true;
        MediawayStatus::InternalPanic
    }
}

/// Register the video track.
///
/// FLV has exactly one video slot (no track-id field in the format itself) — `info.id` is
/// ignored; video/audio are distinguished by which `add_*_track` function was called,
/// matching [`crate::container::mediaway_flv_demuxer_t`]'s fixed stream ids. Only `H264` is
/// a recognized video codec ([`flv::Error::UnsupportedCodec`] otherwise).
///
/// # Safety
///
/// `muxer` must be a live pointer returned by [`mediaway_flv_muxer_create`]. `info` must
/// point to a valid [`MediawayVideoTrackInfo`] whose `extra_data`/`extra_data_len` describe
/// a buffer valid for reads for the duration of this call (or be null with length `0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_flv_muxer_add_video_track(
    muxer: *mut FlvMuxerHandle,
    info: *const MediawayVideoTrackInfo,
) -> MediawayStatus {
    if muxer.is_null() || info.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `muxer` is a valid, live handle pointer (function contract).
    let handle = unsafe { &mut *muxer };
    if handle.poisoned {
        return MediawayStatus::HandlePoisoned;
    }
    // SAFETY: caller guarantees `info` is valid for reads (function contract).
    let info = unsafe { *info };
    // SAFETY: `info.extra_data`/`info.extra_data_len` describe a buffer valid for this call
    // (function contract).
    let Some(extra_data) = (unsafe { borrow_slice(info.extra_data, info.extra_data_len) }) else {
        return MediawayStatus::InvalidArgument;
    };

    let track = StreamInfo::Video {
        id: info.id,
        codec: info.codec.into(),
        time_base: info.time_base.into(),
        geometry: VideoGeometry {
            width: info.width,
            height: info.height,
        },
        extra_data: Bytes::copy_from_slice(extra_data),
    };
    let result = catch_unwind(AssertUnwindSafe(|| {
        handle.inner.add_track(&track).map_err(MediawayStatus::from)
    }));

    match result {
        Ok(Ok(())) => MediawayStatus::Ok,
        Ok(Err(status)) => status,
        Err(_) => {
            handle.poisoned = true;
            MediawayStatus::InternalPanic
        }
    }
}

/// Register the audio track.
///
/// FLV has exactly one audio slot — same `info.id`-ignored reasoning as
/// [`mediaway_flv_muxer_add_video_track`]. `AAC` and `MP3` are the recognized audio codecs
/// ([`flv::Error::UnsupportedCodec`] otherwise).
///
/// # Safety
///
/// Same contract as [`mediaway_flv_muxer_add_video_track`], for [`MediawayAudioTrackInfo`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_flv_muxer_add_audio_track(
    muxer: *mut FlvMuxerHandle,
    info: *const MediawayAudioTrackInfo,
) -> MediawayStatus {
    if muxer.is_null() || info.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `muxer` is a valid, live handle pointer (function contract).
    let handle = unsafe { &mut *muxer };
    if handle.poisoned {
        return MediawayStatus::HandlePoisoned;
    }
    // SAFETY: caller guarantees `info` is valid for reads (function contract).
    let info = unsafe { *info };
    // SAFETY: `info.extra_data`/`info.extra_data_len` describe a buffer valid for this call
    // (function contract).
    let Some(extra_data) = (unsafe { borrow_slice(info.extra_data, info.extra_data_len) }) else {
        return MediawayStatus::InvalidArgument;
    };

    let track = StreamInfo::Audio {
        id: info.id,
        codec: info.codec.into(),
        time_base: info.time_base.into(),
        extra_data: Bytes::copy_from_slice(extra_data),
        sample_rate: info.sample_rate,
        channels: info.channels,
    };
    let result = catch_unwind(AssertUnwindSafe(|| {
        handle.inner.add_track(&track).map_err(MediawayStatus::from)
    }));

    match result {
        Ok(Ok(())) => MediawayStatus::Ok,
        Ok(Err(status)) => status,
        Err(_) => {
            handle.poisoned = true;
            MediawayStatus::InternalPanic
        }
    }
}

/// Mux one packet.
///
/// Writes the track's sequence-header tag first (once, only for codecs that have one) then
/// the data tag, directly into a freshly allocated output buffer — no separate poll step,
/// unlike `mediaway_muxer_push_packet`/`mediaway_muxer_poll_bytes`. The returned buffer
/// must be released with [`crate::container::mediaway_buffer_free`].
///
/// `packet.stream_id` selects video (`0`) vs. audio (`1`) — matching
/// [`mediaway_flv_demuxer_t`]'s fixed stream ids — and must have a matching
/// `add_*_track` call already made, else [`MediawayStatus::UnknownStream`].
///
/// # Safety
///
/// `muxer` must be a live pointer returned by [`mediaway_flv_muxer_create`]. `packet` must
/// point to a valid [`MediawayPacketView`] whose `payload`/`payload_len` describe a buffer
/// valid for reads for the duration of this call (or be null with length `0`). `out_data`
/// and `out_len` must be valid, writable, non-null out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_flv_muxer_push_packet(
    muxer: *mut FlvMuxerHandle,
    packet: *const MediawayPacketView,
    out_data: *mut *mut u8,
    out_len: *mut usize,
) -> MediawayStatus {
    if muxer.is_null() || packet.is_null() || out_data.is_null() || out_len.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `muxer` is a valid, live handle pointer (function contract).
    let handle = unsafe { &mut *muxer };
    if handle.poisoned {
        return MediawayStatus::HandlePoisoned;
    }
    // SAFETY: caller guarantees `packet` is valid for reads (function contract).
    let view = unsafe { *packet };
    // SAFETY: `view.payload`/`view.payload_len` describe a buffer valid for this call
    // (function contract).
    let Some(payload) = (unsafe { borrow_slice(view.payload, view.payload_len) }) else {
        return MediawayStatus::InvalidArgument;
    };

    let packet = Packet {
        stream_id: view.stream_id,
        pts: view.pts,
        dts: view.dts,
        duration: view.duration,
        is_keyframe: view.is_keyframe,
        is_discard: view.is_discard,
        payload: Bytes::copy_from_slice(payload),
    };
    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut buf = Vec::new();
        handle
            .inner
            .push_packet(&packet, &mut buf)
            .map(|()| buf)
            .map_err(MediawayStatus::from)
    }));

    match result {
        Ok(Ok(buf)) => {
            let (ptr, len) = leak_boxed_slice(buf);
            // SAFETY: `out_data`/`out_len` are checked non-null above (function contract).
            unsafe {
                out_data.write(ptr);
                out_len.write(len);
            }
            MediawayStatus::Ok
        }
        Ok(Err(status)) => status,
        Err(_) => {
            handle.poisoned = true;
            MediawayStatus::InternalPanic
        }
    }
}

/// Close and free an FLV muxer handle.
///
/// Always safe to call, including on a poisoned handle. In the unlikely event a panic
/// occurs while dropping the handle, the allocation is deliberately leaked rather than
/// double-handled (`adr/0001-mp4-mux-demux-c-abi.md` §7) — this would itself be a Mediaway
/// bug to fix if ever observed.
///
/// # Safety
///
/// `muxer` must be null or a pointer previously returned by [`mediaway_flv_muxer_create`]
/// and not already passed to this function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_flv_muxer_close(muxer: *mut FlvMuxerHandle) {
    if muxer.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `muxer` is a valid, not-yet-freed handle pointer
        // (function contract).
        drop(unsafe { Box::from_raw(muxer) });
    }));
}
