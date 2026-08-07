//! Opaque MP3 (MPEG Layer III) muxer handle and its C ABI functions.
//!
//! Dedicated shape, not a `MuxerState` variant: `mp3::Muxer` has a fixed header for the
//! session's lifetime (no track registration at all) and `write_frame` takes an explicit
//! `padding` bit the generic `Packet`-based shape has no slot for — real Layer III encoders
//! flip it per frame to average out fractional frame lengths (bit-reservoir accounting).
//! See `adr/0007-mp3-c-abi.md`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mediaway_container::mp3;
use mpeg_audio::{ChannelMode, FrameHeader, MpegVersion};

use crate::container::buffer::{borrow_slice, leak_boxed_slice};
use crate::container::status::MediawayStatus;
use crate::container::types::{MediawayChannelMode, MediawayMp3FrameHeader, MediawayMpegVersion};

/// Opaque MP3 muxer handle (`mediaway_mp3_muxer_t*` in the C header).
///
/// Thread-confined by convention: may be moved between threads, but must not be used from
/// two threads concurrently without external synchronization.
pub struct Mp3MuxerHandle {
    poisoned: bool,
    inner: mp3::Muxer,
}

const fn to_mpeg_version(version: MediawayMpegVersion) -> MpegVersion {
    match version {
        MediawayMpegVersion::Mpeg1 => MpegVersion::Mpeg1,
        MediawayMpegVersion::Mpeg2 => MpegVersion::Mpeg2,
        MediawayMpegVersion::Mpeg25 => MpegVersion::Mpeg25,
    }
}

const fn to_channel_mode(mode: MediawayChannelMode) -> ChannelMode {
    match mode {
        MediawayChannelMode::Stereo => ChannelMode::Stereo,
        MediawayChannelMode::JointStereo => ChannelMode::JointStereo,
        MediawayChannelMode::DualChannel => ChannelMode::DualChannel,
        MediawayChannelMode::Mono => ChannelMode::Mono,
    }
}

/// Open a mux session for `header`.
///
/// Bitrate/sample-rate/channel mode stay constant for the session's lifetime — real Layer
/// III streams this facade targets don't vary them mid-stream (VBR would need a new header
/// per frame, out of scope).
///
/// Returns null for a non-standard bitrate/sample-rate combination
/// ([`mpeg_audio::Error::UnsupportedBitrate`]/[`mpeg_audio::Error::UnsupportedSampleRate`])
/// or if a panic was caught during construction — both collapse to null (no status side
/// channel on this constructor, matching
/// [`crate::container::mediaway_adts_muxer_create`]'s precedent).
///
/// # Safety
///
/// `header` must point to a valid [`MediawayMp3FrameHeader`], valid for reads for the
/// duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_mp3_muxer_create(
    header: *const MediawayMp3FrameHeader,
) -> *mut Mp3MuxerHandle {
    if header.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: caller guarantees `header` is valid for reads (function contract).
    let header = unsafe { *header };
    let header = FrameHeader {
        version: to_mpeg_version(header.version),
        bitrate_kbps: header.bitrate_kbps,
        sample_rate: header.sample_rate,
        channel_mode: to_channel_mode(header.channel_mode),
    };

    let built = catch_unwind(AssertUnwindSafe(|| mp3::Muxer::new(header)));
    match built {
        Ok(Ok(inner)) => Box::into_raw(Box::new(Mp3MuxerHandle {
            poisoned: false,
            inner,
        })),
        Ok(Err(_)) | Err(_) => std::ptr::null_mut(),
    }
}

/// Append one already-encoded Layer III frame body into a freshly allocated output buffer.
///
/// Fails with [`MediawayStatus::InvalidPacket`] when `frame_body`'s length doesn't match
/// what the header's bitrate/sample-rate/`padding` combination requires
/// ([`mpeg_audio::Error::FrameBodyLengthMismatch`]). The returned buffer must be released
/// with [`crate::container::mediaway_buffer_free`].
///
/// # Safety
///
/// `muxer` must be a live pointer returned by [`mediaway_mp3_muxer_create`] and not yet
/// passed to [`mediaway_mp3_muxer_close`]. `frame_body` must be valid for reads of
/// `frame_body_len` bytes for the duration of this call (or be null with length `0`).
/// `out_data` and `out_len` must be valid, writable, non-null out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_mp3_muxer_write_frame(
    muxer: *mut Mp3MuxerHandle,
    frame_body: *const u8,
    frame_body_len: usize,
    padding: bool,
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
    // SAFETY: `frame_body`/`frame_body_len` describe a buffer valid for this call (function
    // contract).
    let Some(frame_body) = (unsafe { borrow_slice(frame_body, frame_body_len) }) else {
        return MediawayStatus::InvalidArgument;
    };

    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut buf = Vec::new();
        handle
            .inner
            .write_frame(frame_body, padding, &mut buf)
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

/// Close and free an MP3 muxer handle.
///
/// Always safe to call, including on a poisoned handle. In the unlikely event a panic
/// occurs while dropping the handle, the allocation is deliberately leaked rather than
/// double-handled (`adr/0001-mp4-mux-demux-c-abi.md` §7) — this would itself be a Mediaway
/// bug to fix if ever observed.
///
/// # Safety
///
/// `muxer` must be null or a pointer previously returned by [`mediaway_mp3_muxer_create`]
/// and not already passed to this function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_mp3_muxer_close(muxer: *mut Mp3MuxerHandle) {
    if muxer.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `muxer` is a valid, not-yet-freed handle pointer
        // (function contract).
        drop(unsafe { Box::from_raw(muxer) });
    }));
}
