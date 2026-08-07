//! Integration: MPEG-TS mux -> demux round trip through `mediaway-ffi`'s dedicated
//! `mediaway_ts_muxer_t`/`mediaway_ts_demuxer_t` C ABI (`adr/0006-mpeg-ts-c-abi.md`) — TS is
//! not reachable through the generic `mediaway_muxer_t`/`mediaway_demuxer_t` handles
//! (elementary streams are registered at construction time, not via `add_track`; the mux
//! writes directly into a caller-supplied buffer with explicit `pts_90k`/`dts_90k` params
//! instead of a `mediaway_packet_view_t`).

#![cfg(all(feature = "mux", feature = "demux"))]
#![allow(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    reason = "integration test"
)]

use mediaway_ffi::container::{
    MediawayCodecKind, MediawayPacket, MediawayRational, MediawayStatus, MediawayStreamInfo,
    MediawayTsElementaryStream, mediaway_buffer_free, mediaway_packet_free,
    mediaway_stream_info_free, mediaway_ts_demuxer_close, mediaway_ts_demuxer_create,
    mediaway_ts_demuxer_finish, mediaway_ts_demuxer_finish_free, mediaway_ts_demuxer_poll_packet,
    mediaway_ts_demuxer_push_bytes, mediaway_ts_demuxer_stream_at,
    mediaway_ts_demuxer_stream_count, mediaway_ts_muxer_close, mediaway_ts_muxer_create,
    mediaway_ts_muxer_write_access_unit, mediaway_ts_muxer_write_pat_pmt,
};

const VIDEO_PID: u16 = 0x100;
const AUDIO_PID: u16 = 0x101;

const fn streams() -> [MediawayTsElementaryStream; 2] {
    [
        MediawayTsElementaryStream {
            pid: VIDEO_PID,
            codec: MediawayCodecKind::H264,
        },
        MediawayTsElementaryStream {
            pid: AUDIO_PID,
            codec: MediawayCodecKind::Aac,
        },
    ]
}

#[test]
fn ts_avc_aac_mux_demux_round_trips_through_ffi() {
    let streams = streams();
    let muxer = unsafe { mediaway_ts_muxer_create(1, 0x1000, streams.as_ptr(), streams.len()) };
    assert!(!muxer.is_null());

    let mut ts_bytes = Vec::new();
    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    let status =
        unsafe { mediaway_ts_muxer_write_pat_pmt(muxer, &raw mut out_data, &raw mut out_len) };
    assert_eq!(status, MediawayStatus::Ok, "write_pat_pmt");
    assert!(out_len > 0);
    // SAFETY: `out_data`/`out_len` were just written by `write_pat_pmt` above.
    ts_bytes.extend_from_slice(unsafe { std::slice::from_raw_parts(out_data, out_len) });
    unsafe { mediaway_buffer_free(out_data, out_len) };

    let video_au: [u8; 6] = [0, 0, 0, 1, 0x65, 0x88];
    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    let status = unsafe {
        mediaway_ts_muxer_write_access_unit(
            muxer,
            VIDEO_PID,
            video_au.as_ptr(),
            video_au.len(),
            90_000,
            false,
            0,
            true,
            &raw mut out_data,
            &raw mut out_len,
        )
    };
    assert_eq!(status, MediawayStatus::Ok, "write_access_unit video");
    assert!(out_len > 0);
    // SAFETY: `out_data`/`out_len` were just written by `write_access_unit` above.
    ts_bytes.extend_from_slice(unsafe { std::slice::from_raw_parts(out_data, out_len) });
    unsafe { mediaway_buffer_free(out_data, out_len) };

    let audio_au: [u8; 4] = [1, 2, 3, 4];
    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    let status = unsafe {
        mediaway_ts_muxer_write_access_unit(
            muxer,
            AUDIO_PID,
            audio_au.as_ptr(),
            audio_au.len(),
            90_500,
            true,
            90_000,
            false,
            &raw mut out_data,
            &raw mut out_len,
        )
    };
    assert_eq!(status, MediawayStatus::Ok, "write_access_unit audio");
    assert!(out_len > 0);
    // SAFETY: `out_data`/`out_len` were just written by `write_access_unit` above.
    ts_bytes.extend_from_slice(unsafe { std::slice::from_raw_parts(out_data, out_len) });
    unsafe { mediaway_buffer_free(out_data, out_len) };

    // A real player needs the *next* PES packet on the same PID (or `finish`) to confirm
    // the previous one's boundary — write a trailing marker AU on the video PID.
    let video_au_2: [u8; 5] = [0, 0, 0, 1, 0x41];
    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    let status = unsafe {
        mediaway_ts_muxer_write_access_unit(
            muxer,
            VIDEO_PID,
            video_au_2.as_ptr(),
            video_au_2.len(),
            90_033,
            false,
            0,
            false,
            &raw mut out_data,
            &raw mut out_len,
        )
    };
    assert_eq!(status, MediawayStatus::Ok, "write_access_unit video 2");
    // SAFETY: `out_data`/`out_len` were just written by `write_access_unit` above.
    ts_bytes.extend_from_slice(unsafe { std::slice::from_raw_parts(out_data, out_len) });
    unsafe { mediaway_buffer_free(out_data, out_len) };

    unsafe { mediaway_ts_muxer_close(muxer) };

    let demuxer = mediaway_ts_demuxer_create();
    assert!(!demuxer.is_null());
    let status =
        unsafe { mediaway_ts_demuxer_push_bytes(demuxer, ts_bytes.as_ptr(), ts_bytes.len()) };
    assert_eq!(status, MediawayStatus::Ok, "push_bytes");

    let mut packet = MediawayPacket {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 0,
        is_keyframe: false,
        is_discard: false,
        payload: std::ptr::null_mut(),
        payload_len: 0,
    };
    let mut has = false;
    let status = unsafe { mediaway_ts_demuxer_poll_packet(demuxer, &raw mut packet, &raw mut has) };
    assert_eq!(status, MediawayStatus::Ok, "poll_packet");
    assert!(has, "expected the video packet to be recovered");
    assert_eq!(packet.stream_id, u32::from(VIDEO_PID));
    assert_eq!(packet.pts, 90_000);
    assert!(packet.is_keyframe);
    assert_eq!(packet.payload_len, video_au.len());
    unsafe { mediaway_packet_free(&raw mut packet) };

    assert_eq!(unsafe { mediaway_ts_demuxer_stream_count(demuxer) }, 2);
    let mut stream_info = MediawayStreamInfo {
        id: 0,
        codec: MediawayCodecKind::H264,
        time_base: MediawayRational { num: 0, den: 1 },
        has_geometry: false,
        width: 0,
        height: 0,
        sample_rate: 0,
        channels: 0,
        extra_data: std::ptr::null_mut(),
        extra_data_len: 0,
    };
    let status = unsafe { mediaway_ts_demuxer_stream_at(demuxer, 0, &raw mut stream_info) };
    assert_eq!(status, MediawayStatus::Ok, "stream_at 0");
    unsafe { mediaway_stream_info_free(&raw mut stream_info) };

    unsafe { mediaway_ts_demuxer_close(demuxer) };
}

/// [`mediaway_ts_demuxer_finish`] force-emits the final pending access unit on a PID with
/// no trailing marker to confirm its PES boundary.
#[test]
fn ts_demuxer_finish_emits_the_final_pending_access_unit() {
    let streams = streams();
    let muxer = unsafe { mediaway_ts_muxer_create(1, 0x1000, streams.as_ptr(), streams.len()) };
    assert!(!muxer.is_null());

    let mut ts_bytes = Vec::new();
    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    unsafe { mediaway_ts_muxer_write_pat_pmt(muxer, &raw mut out_data, &raw mut out_len) };
    ts_bytes.extend_from_slice(unsafe { std::slice::from_raw_parts(out_data, out_len) });
    unsafe { mediaway_buffer_free(out_data, out_len) };

    let video_au: [u8; 3] = [9, 9, 9];
    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    let status = unsafe {
        mediaway_ts_muxer_write_access_unit(
            muxer,
            VIDEO_PID,
            video_au.as_ptr(),
            video_au.len(),
            90_000,
            false,
            0,
            true,
            &raw mut out_data,
            &raw mut out_len,
        )
    };
    assert_eq!(status, MediawayStatus::Ok);
    ts_bytes.extend_from_slice(unsafe { std::slice::from_raw_parts(out_data, out_len) });
    unsafe { mediaway_buffer_free(out_data, out_len) };
    unsafe { mediaway_ts_muxer_close(muxer) };

    let demuxer = mediaway_ts_demuxer_create();
    assert!(!demuxer.is_null());
    unsafe { mediaway_ts_demuxer_push_bytes(demuxer, ts_bytes.as_ptr(), ts_bytes.len()) };

    // No trailing AU on this PID — poll_packet alone can't confirm the PES boundary yet.
    let mut packet = MediawayPacket {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 0,
        is_keyframe: false,
        is_discard: false,
        payload: std::ptr::null_mut(),
        payload_len: 0,
    };
    let mut has = false;
    unsafe { mediaway_ts_demuxer_poll_packet(demuxer, &raw mut packet, &raw mut has) };
    assert!(!has, "no packet should be ready before finish");

    let mut out_packets = std::ptr::null_mut();
    let mut out_count = 0usize;
    let status =
        unsafe { mediaway_ts_demuxer_finish(demuxer, &raw mut out_packets, &raw mut out_count) };
    assert_eq!(status, MediawayStatus::Ok, "finish");
    assert_eq!(out_count, 1);
    // SAFETY: `out_packets`/`out_count` were just written by `finish` above.
    let finished = unsafe { std::slice::from_raw_parts(out_packets, out_count) };
    // SAFETY: `finished[0].payload`/`payload_len` are valid for this read (owned by the
    // still-live array).
    let payload =
        unsafe { std::slice::from_raw_parts(finished[0].payload, finished[0].payload_len) };
    assert_eq!(payload, &video_au[..]);
    unsafe { mediaway_ts_demuxer_finish_free(out_packets, out_count) };

    unsafe { mediaway_ts_demuxer_close(demuxer) };
}

/// A reserved PID (`0`/`1`) is rejected at construction, collapsing to a null handle
/// (`adr/0006-mpeg-ts-c-abi.md` — no status side channel on this constructor).
#[test]
fn ts_muxer_create_rejects_invalid_pid() {
    let bad = [MediawayTsElementaryStream {
        pid: 1, // reserved for CAT
        codec: MediawayCodecKind::H264,
    }];
    let muxer = unsafe { mediaway_ts_muxer_create(1, 0x1000, bad.as_ptr(), bad.len()) };
    assert!(muxer.is_null());
}
