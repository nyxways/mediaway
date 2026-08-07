//! Opaque MPEG-TS muxer handle and its C ABI functions.
//!
//! Dedicated shape, not a `MuxerState` variant: `ts::Muxer::new` takes the full elementary
//! stream list upfront (no `add_track` after construction), and `write_pat_pmt`/
//! `write_access_unit` both write directly into a caller-supplied output buffer like FLV's
//! muxer — plus `write_access_unit` takes raw `pts_90k`/`dts_90k` clock values instead of a
//! [`mediaway_common::Packet`]'s track-timebase `pts`/`dts` (MPEG-TS's 90 kHz system clock
//! is not a per-track choice). See `adr/0006-mpeg-ts-c-abi.md`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mediaway_container::ts::{self, ElementaryStream, StreamType};

use crate::container::buffer::{borrow_slice, leak_boxed_slice};
use crate::container::status::MediawayStatus;
use crate::container::types::{MediawayCodecKind, MediawayTsElementaryStream};

/// Opaque MPEG-TS muxer handle (`mediaway_ts_muxer_t*` in the C header).
///
/// Thread-confined by convention: may be moved between threads, but must not be used from
/// two threads concurrently without external synchronization.
pub struct TsMuxerHandle {
    poisoned: bool,
    inner: ts::Muxer,
}

/// Maps a codec onto the [`StreamType`] this facade can encode, or `None` for anything else
/// (`ts::Muxer` has no [`mediaway_container::CodecKind`]-generic PMT entry).
const fn to_stream_type(codec: MediawayCodecKind) -> Option<StreamType> {
    match codec {
        MediawayCodecKind::H264 => Some(StreamType::H264),
        MediawayCodecKind::Hevc => Some(StreamType::Hevc),
        MediawayCodecKind::Aac => Some(StreamType::Aac),
        MediawayCodecKind::Mp3 => Some(StreamType::Mp3),
        _ => None,
    }
}

/// Start a mux session for one program's elementary streams.
///
/// `pmt_pid` and every stream's `pid` must be in `2..=0x1FFF` (`0`/`1` are reserved for
/// PAT/CAT); every stream's `codec` must map to a [`StreamType`] (`H264`/`Hevc`/`Aac`/
/// `Mp3`). Returns null for an invalid PID, an unsupported codec, or a caught panic during
/// construction — all three collapse to null (no status side channel on this constructor,
/// matching [`crate::container::mediaway_adts_muxer_create`]'s precedent).
///
/// # Safety
///
/// `streams` must be valid for reads of `stream_count` [`MediawayTsElementaryStream`]
/// elements for the duration of this call (or be null with `stream_count == 0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_ts_muxer_create(
    program_number: u16,
    pmt_pid: u16,
    streams: *const MediawayTsElementaryStream,
    stream_count: usize,
) -> *mut TsMuxerHandle {
    if streams.is_null() && stream_count > 0 {
        return std::ptr::null_mut();
    }
    // SAFETY: caller guarantees `streams` is valid for reads of `stream_count` elements, or
    // null with `stream_count == 0` (function contract).
    let inputs = unsafe {
        if streams.is_null() {
            &[]
        } else {
            std::slice::from_raw_parts(streams, stream_count)
        }
    };

    let mut elementary = Vec::with_capacity(inputs.len());
    for input in inputs {
        let Some(stream_type) = to_stream_type(input.codec) else {
            return std::ptr::null_mut();
        };
        elementary.push(ElementaryStream {
            pid: input.pid,
            stream_type,
        });
    }

    let built = catch_unwind(AssertUnwindSafe(|| {
        ts::Muxer::new(program_number, pmt_pid, &elementary)
    }));
    match built {
        Ok(Ok(inner)) => Box::into_raw(Box::new(TsMuxerHandle {
            poisoned: false,
            inner,
        })),
        Ok(Err(_)) | Err(_) => std::ptr::null_mut(),
    }
}

/// Write PAT + PMT packets into a freshly allocated output buffer.
///
/// Call once at the start and periodically thereafter — real players expect PAT/PMT to
/// repeat. The buffer must be released with [`crate::container::mediaway_buffer_free`].
///
/// # Safety
///
/// `muxer` must be a live pointer returned by [`mediaway_ts_muxer_create`]. `out_data` and
/// `out_len` must be valid, writable, non-null out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_ts_muxer_write_pat_pmt(
    muxer: *mut TsMuxerHandle,
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
        handle.inner.write_pat_pmt(&mut buf);
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

/// Packetize one access unit for `pid` into PES + TS packets, written into a freshly
/// allocated output buffer. The buffer must be released with
/// [`crate::container::mediaway_buffer_free`].
///
/// `pts_90k`/`dts_90k` are the real MPEG-TS 90 kHz clock values, not a track's own
/// timebase-relative units — `has_dts == false` means "no DTS" (video streams commonly omit
/// it when PTS == DTS).
///
/// # Safety
///
/// `muxer` must be a live pointer returned by [`mediaway_ts_muxer_create`]. `data` must be
/// valid for reads of `data_len` bytes for the duration of this call (or null with
/// `data_len == 0`). `out_data` and `out_len` must be valid, writable, non-null
/// out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_ts_muxer_write_access_unit(
    muxer: *mut TsMuxerHandle,
    pid: u16,
    data: *const u8,
    data_len: usize,
    pts_90k: u64,
    has_dts: bool,
    dts_90k: u64,
    random_access: bool,
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
    // SAFETY: `data`/`data_len` describe a buffer valid for this call (function contract).
    let Some(access_unit) = (unsafe { borrow_slice(data, data_len) }) else {
        return MediawayStatus::InvalidArgument;
    };
    let dts_90k = has_dts.then_some(dts_90k);

    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut buf = Vec::new();
        handle
            .inner
            .write_access_unit(pid, access_unit, pts_90k, dts_90k, random_access, &mut buf)
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

/// Close and free an MPEG-TS muxer handle.
///
/// Always safe to call, including on a poisoned handle. In the unlikely event a panic
/// occurs while dropping the handle, the allocation is deliberately leaked rather than
/// double-handled (`adr/0001-mp4-mux-demux-c-abi.md` §7) — this would itself be a Mediaway
/// bug to fix if ever observed.
///
/// # Safety
///
/// `muxer` must be null or a pointer previously returned by [`mediaway_ts_muxer_create`]
/// and not already passed to this function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_ts_muxer_close(muxer: *mut TsMuxerHandle) {
    if muxer.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `muxer` is a valid, not-yet-freed handle pointer
        // (function contract).
        drop(unsafe { Box::from_raw(muxer) });
    }));
}
