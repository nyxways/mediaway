//! Integration: MP3 (MPEG Layer III) mux -> demux round trip through `mediaway-ffi`'s
//! dedicated `mediaway_mp3_muxer_t`/`mediaway_mp3_demuxer_t` C ABI
//! (`adr/0007-mp3-c-abi.md`) — MP3 is not reachable through the generic
//! `mediaway_muxer_t`/`mediaway_demuxer_t` handles (fixed header for the session's
//! lifetime, no track registration; `write_frame` takes an explicit `padding` bit no
//! `mediaway_packet_view_t` has a slot for).

#![cfg(all(feature = "mux", feature = "demux"))]
#![allow(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    reason = "integration test"
)]

use mediaway_ffi::container::{
    MediawayChannelMode, MediawayCodecKind, MediawayMp3FrameHeader, MediawayMpegVersion,
    MediawayPacket, MediawayStatus, MediawayStreamInfo, mediaway_buffer_free,
    mediaway_mp3_demuxer_close, mediaway_mp3_demuxer_create, mediaway_mp3_demuxer_poll_packet,
    mediaway_mp3_demuxer_push_bytes, mediaway_mp3_demuxer_stream_at,
    mediaway_mp3_demuxer_stream_count, mediaway_mp3_muxer_close, mediaway_mp3_muxer_create,
    mediaway_mp3_muxer_write_frame, mediaway_packet_free, mediaway_stream_info_free,
};

const fn header() -> MediawayMp3FrameHeader {
    MediawayMp3FrameHeader {
        version: MediawayMpegVersion::Mpeg1,
        bitrate_kbps: 128,
        sample_rate: 44_100,
        channel_mode: MediawayChannelMode::Stereo,
    }
}

// MPEG-1 Layer III frame_len(padding=false) for 128 kbps / 44100 Hz: floor(144_000 * 128 /
// 44100) = 417 bytes total, so the body (header excluded) is 417 - 4 = 413 bytes — mirrors
// `mediaway-container::mp3_tests::mux_then_demux_roundtrips_frame_and_synthesizes_timing`,
// which computes the same value via `FrameHeader::frame_len`.
const FRAME_BODY_LEN: usize = 413;

#[test]
fn mp3_mux_demux_round_trips_through_ffi() {
    let header = header();
    let muxer = unsafe { mediaway_mp3_muxer_create(&raw const header) };
    assert!(!muxer.is_null());

    let body = [0xABu8; FRAME_BODY_LEN];
    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    let status = unsafe {
        mediaway_mp3_muxer_write_frame(
            muxer,
            body.as_ptr(),
            body.len(),
            false,
            &raw mut out_data,
            &raw mut out_len,
        )
    };
    assert_eq!(status, MediawayStatus::Ok, "write_frame");
    assert!(out_len > 0);
    // SAFETY: `out_data`/`out_len` were just written by `write_frame` above.
    let mp3_bytes = unsafe { std::slice::from_raw_parts(out_data, out_len) }.to_vec();
    unsafe { mediaway_buffer_free(out_data, out_len) };
    unsafe { mediaway_mp3_muxer_close(muxer) };

    let demuxer = mediaway_mp3_demuxer_create();
    assert!(!demuxer.is_null());
    let status =
        unsafe { mediaway_mp3_demuxer_push_bytes(demuxer, mp3_bytes.as_ptr(), mp3_bytes.len()) };
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
    let status =
        unsafe { mediaway_mp3_demuxer_poll_packet(demuxer, &raw mut packet, &raw mut has) };
    assert_eq!(status, MediawayStatus::Ok, "poll_packet");
    assert!(has, "expected the frame to be recovered");
    assert_eq!(packet.payload_len, FRAME_BODY_LEN);
    assert_eq!(packet.pts, 0);
    assert_eq!(packet.duration, 1152, "MPEG-1 Layer III samples per frame");
    unsafe { mediaway_packet_free(&raw mut packet) };

    assert_eq!(unsafe { mediaway_mp3_demuxer_stream_count(demuxer) }, 1);
    let mut stream_info = MediawayStreamInfo {
        id: 0,
        codec: MediawayCodecKind::H264,
        time_base: mediaway_ffi::container::MediawayRational { num: 0, den: 1 },
        has_geometry: false,
        width: 0,
        height: 0,
        sample_rate: 0,
        channels: 0,
        extra_data: std::ptr::null_mut(),
        extra_data_len: 0,
    };
    let status = unsafe { mediaway_mp3_demuxer_stream_at(demuxer, 0, &raw mut stream_info) };
    assert_eq!(status, MediawayStatus::Ok, "stream_at 0");
    assert_eq!(stream_info.codec, MediawayCodecKind::Mp3);
    assert_eq!(stream_info.sample_rate, 44_100);
    assert_eq!(stream_info.channels, 2);
    unsafe { mediaway_stream_info_free(&raw mut stream_info) };

    unsafe { mediaway_mp3_demuxer_close(demuxer) };
}

/// A mono header reports one channel on the demuxed stream.
#[test]
fn mp3_mono_stream_reports_one_channel() {
    let header = MediawayMp3FrameHeader {
        channel_mode: MediawayChannelMode::Mono,
        ..header()
    };
    let muxer = unsafe { mediaway_mp3_muxer_create(&raw const header) };
    assert!(!muxer.is_null());

    let body = [0u8; FRAME_BODY_LEN];
    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    let status = unsafe {
        mediaway_mp3_muxer_write_frame(
            muxer,
            body.as_ptr(),
            body.len(),
            false,
            &raw mut out_data,
            &raw mut out_len,
        )
    };
    assert_eq!(status, MediawayStatus::Ok);
    // SAFETY: `out_data`/`out_len` were just written by `write_frame` above.
    let mp3_bytes = unsafe { std::slice::from_raw_parts(out_data, out_len) }.to_vec();
    unsafe { mediaway_buffer_free(out_data, out_len) };
    unsafe { mediaway_mp3_muxer_close(muxer) };

    let demuxer = mediaway_mp3_demuxer_create();
    unsafe { mediaway_mp3_demuxer_push_bytes(demuxer, mp3_bytes.as_ptr(), mp3_bytes.len()) };
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
    unsafe { mediaway_mp3_demuxer_poll_packet(demuxer, &raw mut packet, &raw mut has) };
    assert!(has);
    unsafe { mediaway_packet_free(&raw mut packet) };

    let mut stream_info = MediawayStreamInfo {
        id: 0,
        codec: MediawayCodecKind::Mp3,
        time_base: mediaway_ffi::container::MediawayRational { num: 0, den: 1 },
        has_geometry: false,
        width: 0,
        height: 0,
        sample_rate: 0,
        channels: 0,
        extra_data: std::ptr::null_mut(),
        extra_data_len: 0,
    };
    unsafe { mediaway_mp3_demuxer_stream_at(demuxer, 0, &raw mut stream_info) };
    assert_eq!(stream_info.channels, 1);
    unsafe { mediaway_stream_info_free(&raw mut stream_info) };
    unsafe { mediaway_mp3_demuxer_close(demuxer) };
}

/// A frame body of the wrong length for the header's bitrate/sample-rate/padding
/// combination is rejected honestly, not silently truncated/padded.
#[test]
fn mp3_muxer_rejects_wrong_frame_body_length() {
    let header = header();
    let muxer = unsafe { mediaway_mp3_muxer_create(&raw const header) };
    assert!(!muxer.is_null());

    let wrong_body = [0u8; 10];
    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    let status = unsafe {
        mediaway_mp3_muxer_write_frame(
            muxer,
            wrong_body.as_ptr(),
            wrong_body.len(),
            false,
            &raw mut out_data,
            &raw mut out_len,
        )
    };
    assert_eq!(status, MediawayStatus::InvalidPacket);
    unsafe { mediaway_mp3_muxer_close(muxer) };
}
