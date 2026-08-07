//! Integration: Ogg and ADTS mux -> demux round trips through `mediaway-ffi`'s dedicated
//! `mediaway_ogg_muxer_t`/`mediaway_ogg_demuxer_t`/`mediaway_adts_muxer_t`/
//! `mediaway_adts_demuxer_t` C ABI (`adr/0004-ogg-adts-c-abi.md`) — neither format is
//! reachable through the generic `mediaway_muxer_t`/`mediaway_demuxer_t` handles (no track
//! registration, no `Open`/`Live` typestate).

#![cfg(all(feature = "mux", feature = "demux"))]
#![allow(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    reason = "integration test"
)]

use mediaway_ffi::container::{
    MediawayCodecKind, MediawayPacket, MediawayPacketView, MediawayRational, MediawayStatus,
    MediawayStreamInfo, mediaway_adts_demuxer_close, mediaway_adts_demuxer_create,
    mediaway_adts_demuxer_poll_packet, mediaway_adts_demuxer_push_bytes,
    mediaway_adts_demuxer_stream_at, mediaway_adts_demuxer_stream_count, mediaway_adts_muxer_close,
    mediaway_adts_muxer_create, mediaway_adts_muxer_flush, mediaway_adts_muxer_poll_bytes,
    mediaway_adts_muxer_push_packet, mediaway_buffer_free, mediaway_ogg_demuxer_close,
    mediaway_ogg_demuxer_create, mediaway_ogg_demuxer_poll_packet, mediaway_ogg_demuxer_push_bytes,
    mediaway_ogg_demuxer_stream_at, mediaway_ogg_demuxer_stream_count, mediaway_ogg_muxer_close,
    mediaway_ogg_muxer_create, mediaway_ogg_muxer_flush, mediaway_ogg_muxer_poll_bytes,
    mediaway_ogg_muxer_push_packet, mediaway_packet_free, mediaway_stream_info_free,
};

/// `OpusHead` identification-header payload (RFC 7845 §5.1) recognized by
/// `mediaway-container::ogg`'s `identify()` — same construction as that crate's own
/// `ogg_tests.rs::opus_head`.
fn opus_head(channels: u8) -> Vec<u8> {
    let mut h = Vec::new();
    h.extend_from_slice(b"OpusHead");
    h.push(1); // version
    h.push(channels);
    h.extend_from_slice(&0u16.to_le_bytes()); // pre-skip
    h.extend_from_slice(&48_000u32.to_le_bytes()); // input sample rate (informational)
    h.extend_from_slice(&0i16.to_le_bytes()); // output gain
    h.push(0); // channel mapping family
    h
}

fn push_and_drain(
    muxer: *mut mediaway_ffi::container::OggMuxerHandle,
    view: &MediawayPacketView,
    out: &mut Vec<u8>,
) {
    let status = unsafe { mediaway_ogg_muxer_push_packet(muxer, view) };
    assert_eq!(status, MediawayStatus::Ok, "push_packet");
    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    let status =
        unsafe { mediaway_ogg_muxer_poll_bytes(muxer, &raw mut out_data, &raw mut out_len) };
    assert_eq!(status, MediawayStatus::Ok, "poll_bytes");
    if out_len > 0 {
        // SAFETY: `out_data`/`out_len` were just written by `poll_bytes` above.
        out.extend_from_slice(unsafe { std::slice::from_raw_parts(out_data, out_len) });
        unsafe { mediaway_buffer_free(out_data, out_len) };
    }
}

#[test]
fn ogg_opus_mux_demux_round_trips_through_ffi() {
    // ── mux: identification header, then one Opus audio packet ────────────
    let muxer = mediaway_ogg_muxer_create(1);
    assert!(!muxer.is_null());

    let head = opus_head(2);
    let mut ogg_bytes = Vec::new();
    push_and_drain(
        muxer,
        &MediawayPacketView {
            stream_id: 0,
            pts: 0,
            dts: 0,
            duration: 0,
            is_keyframe: true,
            is_discard: false,
            payload: head.as_ptr(),
            payload_len: head.len(),
        },
        &mut ogg_bytes,
    );
    let audio: [u8; 4] = [1, 2, 3, 4];
    push_and_drain(
        muxer,
        &MediawayPacketView {
            stream_id: 0,
            pts: 960,
            dts: 960,
            duration: 0,
            is_keyframe: true,
            is_discard: false,
            payload: audio.as_ptr(),
            payload_len: audio.len(),
        },
        &mut ogg_bytes,
    );
    let status = unsafe { mediaway_ogg_muxer_flush(muxer) };
    assert_eq!(status, MediawayStatus::Ok, "flush");
    unsafe { mediaway_ogg_muxer_close(muxer) };

    assert!(!ogg_bytes.is_empty(), "expected non-empty Ogg bytes");
    assert_eq!(&ogg_bytes[0..4], b"OggS", "Ogg capture pattern");

    // ── demux: recover the identified stream + the audio packet ───────────
    let demuxer = mediaway_ogg_demuxer_create();
    assert!(!demuxer.is_null());
    let status =
        unsafe { mediaway_ogg_demuxer_push_bytes(demuxer, ogg_bytes.as_ptr(), ogg_bytes.len()) };
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
        unsafe { mediaway_ogg_demuxer_poll_packet(demuxer, &raw mut packet, &raw mut has) };
    assert_eq!(status, MediawayStatus::Ok, "poll_packet");
    assert!(has, "expected the audio packet to be recovered");
    assert_eq!(packet.payload_len, 4);
    assert_eq!(packet.pts, 960);
    unsafe { mediaway_packet_free(&raw mut packet) };

    assert_eq!(unsafe { mediaway_ogg_demuxer_stream_count(demuxer) }, 1);
    let mut stream_info = MediawayStreamInfo {
        id: 0,
        codec: MediawayCodecKind::Aac,
        time_base: MediawayRational { num: 0, den: 1 },
        has_geometry: false,
        width: 0,
        height: 0,
        sample_rate: 0,
        channels: 0,
        extra_data: std::ptr::null_mut(),
        extra_data_len: 0,
    };
    let status = unsafe { mediaway_ogg_demuxer_stream_at(demuxer, 0, &raw mut stream_info) };
    assert_eq!(status, MediawayStatus::Ok, "stream_at");
    assert_eq!(stream_info.codec, MediawayCodecKind::Opus);
    assert_eq!(stream_info.channels, 2);
    unsafe { mediaway_stream_info_free(&raw mut stream_info) };

    unsafe { mediaway_ogg_demuxer_close(demuxer) };
}

#[test]
fn adts_mux_demux_round_trips_through_ffi() {
    // ── mux: two raw AAC frames, no track registration ─────────────────────
    let muxer = mediaway_adts_muxer_create(44_100, 2);
    assert!(!muxer.is_null());

    let raw_aac = [0xABu8; 100];
    let mut adts_bytes = Vec::new();
    for _ in 0..2 {
        let view = MediawayPacketView {
            stream_id: 0,
            pts: 0,
            dts: 0,
            duration: 0,
            is_keyframe: true,
            is_discard: false,
            payload: raw_aac.as_ptr(),
            payload_len: raw_aac.len(),
        };
        let status = unsafe { mediaway_adts_muxer_push_packet(muxer, &raw const view) };
        assert_eq!(status, MediawayStatus::Ok, "push_packet");
    }
    let status = unsafe { mediaway_adts_muxer_flush(muxer) };
    assert_eq!(status, MediawayStatus::Ok, "flush");
    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    let status =
        unsafe { mediaway_adts_muxer_poll_bytes(muxer, &raw mut out_data, &raw mut out_len) };
    assert_eq!(status, MediawayStatus::Ok, "poll_bytes");
    assert!(out_len > 0, "expected non-empty ADTS bytes");
    // SAFETY: `out_data`/`out_len` were just written by `poll_bytes` above.
    adts_bytes.extend_from_slice(unsafe { std::slice::from_raw_parts(out_data, out_len) });
    unsafe { mediaway_buffer_free(out_data, out_len) };
    unsafe { mediaway_adts_muxer_close(muxer) };

    // ADTS sync word: 12 set bits (0xFFF) in the first 12 bits of the frame header.
    assert_eq!(adts_bytes[0], 0xFF);
    assert_eq!(adts_bytes[1] & 0xF0, 0xF0);

    // ── demux: recover both frames, synthesized pts advancing by 1024 samples ──
    let demuxer = mediaway_adts_demuxer_create();
    assert!(!demuxer.is_null());
    let status =
        unsafe { mediaway_adts_demuxer_push_bytes(demuxer, adts_bytes.as_ptr(), adts_bytes.len()) };
    assert_eq!(status, MediawayStatus::Ok, "push_bytes");

    let mut expected_pts = 0i64;
    for _ in 0..2 {
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
            unsafe { mediaway_adts_demuxer_poll_packet(demuxer, &raw mut packet, &raw mut has) };
        assert_eq!(status, MediawayStatus::Ok, "poll_packet");
        assert!(has, "expected a frame to be recovered");
        assert_eq!(packet.payload_len, 100);
        assert_eq!(packet.pts, expected_pts);
        assert_eq!(packet.duration, 1024);
        expected_pts += 1024;
        unsafe { mediaway_packet_free(&raw mut packet) };
    }

    assert_eq!(unsafe { mediaway_adts_demuxer_stream_count(demuxer) }, 1);
    let mut stream_info = MediawayStreamInfo {
        id: 0,
        codec: MediawayCodecKind::Opus,
        time_base: MediawayRational { num: 0, den: 1 },
        has_geometry: false,
        width: 0,
        height: 0,
        sample_rate: 0,
        channels: 0,
        extra_data: std::ptr::null_mut(),
        extra_data_len: 0,
    };
    let status = unsafe { mediaway_adts_demuxer_stream_at(demuxer, 0, &raw mut stream_info) };
    assert_eq!(status, MediawayStatus::Ok, "stream_at");
    assert_eq!(stream_info.codec, MediawayCodecKind::Aac);
    assert_eq!(stream_info.sample_rate, 44_100);
    assert_eq!(stream_info.channels, 2);
    unsafe { mediaway_stream_info_free(&raw mut stream_info) };

    unsafe { mediaway_adts_demuxer_close(demuxer) };
}

/// A non-standard sample rate is rejected at construction, collapsing to a null handle
/// (`adr/0004-ogg-adts-c-abi.md` — no status side channel on this constructor).
#[test]
fn adts_muxer_create_rejects_unsupported_sample_rate() {
    let muxer = mediaway_adts_muxer_create(12_345, 2);
    assert!(muxer.is_null());
}
