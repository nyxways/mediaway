//! Integration: `WebM` mux → demux round trip through `mediaway-ffi`'s C ABI, exercising
//! `mediaway_muxer_create_for_format`/`mediaway_demuxer_create_for_format`
//! (`adr/0003-multi-format-c-abi.md`) — the first format beyond MP4 reachable from the
//! generic `mediaway_muxer_t`/`mediaway_demuxer_t` handles.

#![cfg(all(feature = "mux", feature = "demux"))]
#![allow(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    reason = "integration test"
)]

use mediaway_ffi::container::{
    MediawayCodecKind, MediawayContainerFormat, MediawayPacket, MediawayPacketView,
    MediawayRational, MediawayStatus, MediawayStreamInfo, MediawayVideoTrackInfo,
    mediaway_demuxer_close, mediaway_demuxer_create_for_format, mediaway_demuxer_poll_packet,
    mediaway_demuxer_push_bytes, mediaway_demuxer_stream_at, mediaway_demuxer_stream_count,
    mediaway_muxer_add_video_track, mediaway_muxer_begin, mediaway_muxer_close,
    mediaway_muxer_create_for_format, mediaway_muxer_flush, mediaway_muxer_poll_bytes,
    mediaway_muxer_push_packet, mediaway_packet_free, mediaway_stream_info_free,
};

const FRAME_COUNT: i64 = 5;

#[test]
fn webm_mux_demux_round_trips_through_ffi() {
    // ── mux: register one VP8 video track, push synthetic frames ──────────
    let muxer = mediaway_muxer_create_for_format(MediawayContainerFormat::Webm);
    assert!(!muxer.is_null());

    let video_track = MediawayVideoTrackInfo {
        id: 1,
        codec: MediawayCodecKind::Vp8,
        time_base: MediawayRational { num: 1, den: 30 },
        width: 64,
        height: 64,
        extra_data: std::ptr::null(),
        extra_data_len: 0,
    };
    let status = unsafe { mediaway_muxer_add_video_track(muxer, &raw const video_track) };
    assert_eq!(status, MediawayStatus::Ok, "add_video_track");

    let status = unsafe { mediaway_muxer_begin(muxer) };
    assert_eq!(status, MediawayStatus::Ok, "begin");

    let mut webm_bytes = Vec::new();
    for i in 0..FRAME_COUNT {
        let payload = [0xAAu8; 16];
        let view = MediawayPacketView {
            stream_id: 1,
            pts: i,
            dts: i,
            duration: 1,
            is_keyframe: i == 0,
            is_discard: false,
            payload: payload.as_ptr(),
            payload_len: payload.len(),
        };
        let status = unsafe { mediaway_muxer_push_packet(muxer, &raw const view) };
        assert_eq!(status, MediawayStatus::Ok, "push_packet {i}");

        let mut out_data = std::ptr::null_mut();
        let mut out_len = 0usize;
        let status =
            unsafe { mediaway_muxer_poll_bytes(muxer, &raw mut out_data, &raw mut out_len) };
        assert_eq!(status, MediawayStatus::Ok, "poll_bytes {i}");
        if out_len > 0 {
            // SAFETY: `out_data`/`out_len` were just written by `poll_bytes` above.
            webm_bytes.extend_from_slice(unsafe { std::slice::from_raw_parts(out_data, out_len) });
            unsafe { mediaway_ffi::container::mediaway_buffer_free(out_data, out_len) };
        }
    }
    let status = unsafe { mediaway_muxer_flush(muxer) };
    assert_eq!(status, MediawayStatus::Ok, "flush");
    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    let status = unsafe { mediaway_muxer_poll_bytes(muxer, &raw mut out_data, &raw mut out_len) };
    assert_eq!(status, MediawayStatus::Ok, "final poll_bytes");
    if out_len > 0 {
        // SAFETY: `out_data`/`out_len` were just written by `poll_bytes` above.
        webm_bytes.extend_from_slice(unsafe { std::slice::from_raw_parts(out_data, out_len) });
        unsafe { mediaway_ffi::container::mediaway_buffer_free(out_data, out_len) };
    }
    unsafe { mediaway_muxer_close(muxer) };

    assert!(!webm_bytes.is_empty(), "expected non-empty WebM bytes");
    assert_eq!(&webm_bytes[0..4], [0x1A, 0x45, 0xDF, 0xA3], "EBML magic");

    // ── demux: recover every pushed frame ──────────────────────────────────
    let demuxer = mediaway_demuxer_create_for_format(MediawayContainerFormat::Webm);
    assert!(!demuxer.is_null());
    let status =
        unsafe { mediaway_demuxer_push_bytes(demuxer, webm_bytes.as_ptr(), webm_bytes.len()) };
    assert_eq!(status, MediawayStatus::Ok, "push_bytes");
    assert_eq!(unsafe { mediaway_demuxer_stream_count(demuxer) }, 1);

    let mut stream_info = MediawayStreamInfo {
        id: 0,
        codec: MediawayCodecKind::Vp8,
        time_base: MediawayRational { num: 0, den: 1 },
        has_geometry: false,
        width: 0,
        height: 0,
        sample_rate: 0,
        channels: 0,
        extra_data: std::ptr::null_mut(),
        extra_data_len: 0,
    };
    let status = unsafe { mediaway_demuxer_stream_at(demuxer, 0, &raw mut stream_info) };
    assert_eq!(status, MediawayStatus::Ok, "stream_at");
    assert_eq!(stream_info.codec, MediawayCodecKind::Vp8);
    unsafe { mediaway_stream_info_free(&raw mut stream_info) };

    let mut demuxed_count: i64 = 0;
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
            unsafe { mediaway_demuxer_poll_packet(demuxer, &raw mut packet, &raw mut has) };
        assert_eq!(status, MediawayStatus::Ok, "poll_packet");
        if !has {
            break;
        }
        assert_eq!(packet.payload_len, 16);
        demuxed_count += 1;
        unsafe { mediaway_packet_free(&raw mut packet) };
    }
    assert_eq!(
        demuxed_count, FRAME_COUNT,
        "every pushed frame must demux back"
    );

    unsafe { mediaway_demuxer_close(demuxer) };
}

/// A WebM-backed demuxer has no `ClearKey` support (`adr/0003-multi-format-c-abi.md`) —
/// `set_decryption_key` must fail honestly, not silently no-op.
#[test]
fn webm_demuxer_rejects_set_decryption_key() {
    let demuxer = mediaway_demuxer_create_for_format(MediawayContainerFormat::Webm);
    assert!(!demuxer.is_null());
    let key = [0u8; 16];
    let status = unsafe {
        mediaway_ffi::container::mediaway_demuxer_set_decryption_key(
            demuxer,
            key.as_ptr(),
            key.len(),
        )
    };
    assert_eq!(status, MediawayStatus::InvalidState);
    unsafe { mediaway_demuxer_close(demuxer) };
}
