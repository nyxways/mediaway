//! Integration: FLV mux -> demux round trip through `mediaway-ffi`'s dedicated
//! `mediaway_flv_muxer_t`/`mediaway_flv_demuxer_t` C ABI (`adr/0005-flv-c-abi.md`) — FLV is
//! not reachable through the generic `mediaway_muxer_t`/`mediaway_demuxer_t` handles
//! (`push_packet`/`write_header` write directly into a caller-supplied buffer instead of a
//! separate `poll_bytes` step, and track registration is a fixed one-video/one-audio slot).

#![cfg(all(feature = "mux", feature = "demux"))]
#![allow(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    reason = "integration test"
)]

use mediaway_ffi::container::{
    MediawayAudioTrackInfo, MediawayCodecKind, MediawayPacket, MediawayPacketView,
    MediawayRational, MediawayStatus, MediawayStreamInfo, MediawayVideoTrackInfo,
    mediaway_buffer_free, mediaway_flv_demuxer_close, mediaway_flv_demuxer_create,
    mediaway_flv_demuxer_poll_packet, mediaway_flv_demuxer_push_bytes,
    mediaway_flv_demuxer_stream_at, mediaway_flv_demuxer_stream_count,
    mediaway_flv_muxer_add_audio_track, mediaway_flv_muxer_add_video_track,
    mediaway_flv_muxer_close, mediaway_flv_muxer_create, mediaway_flv_muxer_push_packet,
    mediaway_flv_muxer_write_header, mediaway_packet_free, mediaway_stream_info_free,
};

const MS_TIME_BASE: MediawayRational = MediawayRational { num: 1, den: 1_000 };

#[test]
fn flv_avc_aac_mux_demux_round_trips_through_ffi() {
    // ── mux: one video track (AVC), one audio track (AAC) ──────────────────
    let muxer = mediaway_flv_muxer_create();
    assert!(!muxer.is_null());

    let avcc: [u8; 8] = [1, 0x42, 0, 0x1e, 0xff, 0xe1, 0, 0];
    let video_track = MediawayVideoTrackInfo {
        id: 0,
        codec: MediawayCodecKind::H264,
        time_base: MS_TIME_BASE,
        width: 1280,
        height: 720,
        extra_data: avcc.as_ptr(),
        extra_data_len: avcc.len(),
    };
    let status = unsafe { mediaway_flv_muxer_add_video_track(muxer, &raw const video_track) };
    assert_eq!(status, MediawayStatus::Ok, "add_video_track");

    let asc: [u8; 2] = [0x12, 0x10];
    let audio_track = MediawayAudioTrackInfo {
        id: 0,
        codec: MediawayCodecKind::Aac,
        time_base: MS_TIME_BASE,
        sample_rate: 44_100,
        channels: 2,
        extra_data: asc.as_ptr(),
        extra_data_len: asc.len(),
    };
    let status = unsafe { mediaway_flv_muxer_add_audio_track(muxer, &raw const audio_track) };
    assert_eq!(status, MediawayStatus::Ok, "add_audio_track");

    let mut flv_bytes = Vec::new();
    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    let status = unsafe {
        mediaway_flv_muxer_write_header(muxer, true, true, &raw mut out_data, &raw mut out_len)
    };
    assert_eq!(status, MediawayStatus::Ok, "write_header");
    assert!(out_len > 0, "expected non-empty FLV header");
    // SAFETY: `out_data`/`out_len` were just written by `write_header` above.
    flv_bytes.extend_from_slice(unsafe { std::slice::from_raw_parts(out_data, out_len) });
    unsafe { mediaway_buffer_free(out_data, out_len) };

    let video_payload: [u8; 6] = [0, 0, 0, 2, 0x65, 0x88];
    let video_view = MediawayPacketView {
        stream_id: 0,
        pts: 45,
        dts: 33,
        duration: 0,
        is_keyframe: true,
        is_discard: false,
        payload: video_payload.as_ptr(),
        payload_len: video_payload.len(),
    };
    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    let status = unsafe {
        mediaway_flv_muxer_push_packet(
            muxer,
            &raw const video_view,
            &raw mut out_data,
            &raw mut out_len,
        )
    };
    assert_eq!(status, MediawayStatus::Ok, "push_packet video");
    assert!(out_len > 0);
    // SAFETY: `out_data`/`out_len` were just written by `push_packet` above.
    flv_bytes.extend_from_slice(unsafe { std::slice::from_raw_parts(out_data, out_len) });
    unsafe { mediaway_buffer_free(out_data, out_len) };

    let audio_payload: [u8; 4] = [1, 2, 3, 4];
    let audio_view = MediawayPacketView {
        stream_id: 1,
        pts: 23,
        dts: 23,
        duration: 0,
        is_keyframe: true,
        is_discard: false,
        payload: audio_payload.as_ptr(),
        payload_len: audio_payload.len(),
    };
    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    let status = unsafe {
        mediaway_flv_muxer_push_packet(
            muxer,
            &raw const audio_view,
            &raw mut out_data,
            &raw mut out_len,
        )
    };
    assert_eq!(status, MediawayStatus::Ok, "push_packet audio");
    assert!(out_len > 0);
    // SAFETY: `out_data`/`out_len` were just written by `push_packet` above.
    flv_bytes.extend_from_slice(unsafe { std::slice::from_raw_parts(out_data, out_len) });
    unsafe { mediaway_buffer_free(out_data, out_len) };

    unsafe { mediaway_flv_muxer_close(muxer) };

    assert_eq!(&flv_bytes[0..3], b"FLV", "FLV file signature");

    // ── demux: recover both tracks + the pushed packets ─────────────────────
    let demuxer = mediaway_flv_demuxer_create();
    assert!(!demuxer.is_null());
    let status =
        unsafe { mediaway_flv_demuxer_push_bytes(demuxer, flv_bytes.as_ptr(), flv_bytes.len()) };
    assert_eq!(status, MediawayStatus::Ok, "push_bytes");

    let mut got_video = false;
    let mut got_audio = false;
    loop {
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
            unsafe { mediaway_flv_demuxer_poll_packet(demuxer, &raw mut packet, &raw mut has) };
        assert_eq!(status, MediawayStatus::Ok, "poll_packet");
        if !has {
            break;
        }
        if packet.stream_id == 0 {
            assert_eq!(packet.payload_len, video_payload.len());
            assert_eq!(packet.dts, 33);
            assert_eq!(packet.pts, 45);
            assert!(packet.is_keyframe);
            got_video = true;
        } else {
            assert_eq!(packet.payload_len, audio_payload.len());
            assert_eq!(packet.pts, 23);
            got_audio = true;
        }
        unsafe { mediaway_packet_free(&raw mut packet) };
    }
    assert!(got_video && got_audio, "expected both tracks recovered");

    assert_eq!(unsafe { mediaway_flv_demuxer_stream_count(demuxer) }, 2);
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
    let status = unsafe { mediaway_flv_demuxer_stream_at(demuxer, 0, &raw mut stream_info) };
    assert_eq!(status, MediawayStatus::Ok, "stream_at 0");
    assert_eq!(stream_info.codec, MediawayCodecKind::H264);
    unsafe { mediaway_stream_info_free(&raw mut stream_info) };

    unsafe { mediaway_flv_demuxer_close(demuxer) };
}

/// `add_video_track`/`add_audio_track` reject a codec with no FLV tag-header mapping (see
/// `mediaway-container::flv` module docs on codec coverage).
#[test]
fn flv_muxer_rejects_unsupported_video_codec() {
    let muxer = mediaway_flv_muxer_create();
    assert!(!muxer.is_null());
    let track = MediawayVideoTrackInfo {
        id: 0,
        codec: MediawayCodecKind::Hevc,
        time_base: MS_TIME_BASE,
        width: 0,
        height: 0,
        extra_data: std::ptr::null(),
        extra_data_len: 0,
    };
    let status = unsafe { mediaway_flv_muxer_add_video_track(muxer, &raw const track) };
    assert_eq!(status, MediawayStatus::UnsupportedCodec);
    unsafe { mediaway_flv_muxer_close(muxer) };
}

/// `push_packet` on a `stream_id` with no matching `add_*_track` call fails honestly.
#[test]
fn flv_muxer_rejects_unregistered_stream() {
    let muxer = mediaway_flv_muxer_create();
    assert!(!muxer.is_null());
    let payload: [u8; 5] = [0, 0, 0, 1, 0x65];
    let view = MediawayPacketView {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 0,
        is_keyframe: true,
        is_discard: false,
        payload: payload.as_ptr(),
        payload_len: payload.len(),
    };
    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    let status = unsafe {
        mediaway_flv_muxer_push_packet(muxer, &raw const view, &raw mut out_data, &raw mut out_len)
    };
    assert_eq!(status, MediawayStatus::UnknownStream);
    unsafe { mediaway_flv_muxer_close(muxer) };
}
