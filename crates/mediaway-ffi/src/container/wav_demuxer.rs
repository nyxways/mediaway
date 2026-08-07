//! WAV (RIFF/WAVE PCM) parse — a one-shot function, not a demuxer handle.
//!
//! `mediaway_container::wav::parse` takes a complete in-memory buffer and returns the whole
//! stream's `StreamInfo`/`Packet` in one call — there is no streaming `push_bytes`/
//! `poll_packet` state to hold (RIFF chunk sizes are read from the header up front, not
//! discovered incrementally), so unlike every other format in this crate, WAV demux has no
//! `mediaway_wav_demuxer_t` handle at all. See `adr/0008-wav-c-abi.md`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mediaway_container::wav;

use crate::container::buffer::{borrow_slice, leak_boxed_slice};
use crate::container::status::MediawayStatus;
use crate::container::types::{MediawayPacket, MediawayStreamInfo};

/// Parse a complete RIFF/WAVE buffer into its single track's stream info and one packet
/// holding the whole PCM payload (RIFF/WAVE carries no internal frame boundaries).
///
/// On success, both `*out_info` and `*out_packet` must be released with
/// [`crate::container::mediaway_stream_info_free`]/[`crate::container::mediaway_packet_free`]
/// respectively. On failure, neither out-parameter is written.
///
/// # Safety
///
/// `data` must be valid for reads of `data_len` bytes for the duration of this call (or
/// null with `data_len == 0`). `out_info` and `out_packet` must be valid, writable,
/// non-null out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_wav_parse(
    data: *const u8,
    data_len: usize,
    out_info: *mut MediawayStreamInfo,
    out_packet: *mut MediawayPacket,
) -> MediawayStatus {
    if out_info.is_null() || out_packet.is_null() {
        return MediawayStatus::InvalidArgument;
    }
    // SAFETY: `data`/`data_len` describe a buffer valid for this call (function contract).
    let Some(bytes) = (unsafe { borrow_slice(data, data_len) }) else {
        return MediawayStatus::InvalidArgument;
    };

    let result = catch_unwind(AssertUnwindSafe(|| {
        wav::parse(bytes).map_err(MediawayStatus::from)
    }));

    match result {
        Ok(Ok((stream, packet))) => {
            let geometry = stream.geometry();
            let (stream_extra_data, stream_extra_data_len) =
                leak_boxed_slice(stream.extra_data().to_vec());
            let info = MediawayStreamInfo {
                id: stream.id(),
                codec: stream.codec().into(),
                time_base: stream.time_base().into(),
                has_geometry: geometry.is_some(),
                width: geometry.map_or(0, |g| g.width),
                height: geometry.map_or(0, |g| g.height),
                sample_rate: stream.sample_rate().unwrap_or(0),
                channels: stream.channels().unwrap_or(0),
                extra_data: stream_extra_data,
                extra_data_len: stream_extra_data_len,
            };
            let (payload, payload_len) = leak_boxed_slice(packet.payload.to_vec());
            let packet = MediawayPacket {
                stream_id: packet.stream_id,
                pts: packet.pts,
                dts: packet.dts,
                duration: packet.duration,
                is_keyframe: packet.is_keyframe,
                is_discard: packet.is_discard,
                payload,
                payload_len,
            };
            // SAFETY: `out_info`/`out_packet` are checked non-null above (function
            // contract).
            unsafe {
                out_info.write(info);
                out_packet.write(packet);
            }
            MediawayStatus::Ok
        }
        Ok(Err(status)) => status,
        // No handle to poison — this is a one-shot function, not a stateful demuxer.
        Err(_) => MediawayStatus::InternalPanic,
    }
}
