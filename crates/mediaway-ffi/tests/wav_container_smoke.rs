//! Integration: WAV (RIFF/WAVE PCM) mux -> parse round trip through `mediaway-ffi`'s
//! dedicated `mediaway_wav_muxer_t` handle + one-shot `mediaway_wav_parse` function
//! (`adr/0008-wav-c-abi.md`) — WAV is not reachable through the generic
//! `mediaway_muxer_t`/`mediaway_demuxer_t` handles (`finish` consumes the muxer by value;
//! demux is a single whole-buffer function, not a streaming handle at all).

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
    MediawayStreamInfo, MediawayWavSampleFormat, MediawayWaveFormat, mediaway_buffer_free,
    mediaway_packet_free, mediaway_stream_info_free, mediaway_wav_muxer_close,
    mediaway_wav_muxer_create, mediaway_wav_muxer_create_with_format, mediaway_wav_muxer_finish,
    mediaway_wav_muxer_push_packet, mediaway_wav_parse,
};

#[test]
fn wav_mux_parse_round_trips_through_ffi() {
    let muxer = mediaway_wav_muxer_create(44_100, 2, 16);
    assert!(!muxer.is_null());

    // 2 frames of 4-byte (2ch x 16-bit) PCM.
    let pcm: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let view = MediawayPacketView {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 0,
        is_keyframe: true,
        is_discard: false,
        payload: pcm.as_ptr(),
        payload_len: pcm.len(),
    };
    let status = unsafe { mediaway_wav_muxer_push_packet(muxer, &raw const view) };
    assert_eq!(status, MediawayStatus::Ok, "push_packet");

    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    let status = unsafe { mediaway_wav_muxer_finish(muxer, &raw mut out_data, &raw mut out_len) };
    assert_eq!(status, MediawayStatus::Ok, "finish");
    assert!(out_len > 0);
    // SAFETY: `out_data`/`out_len` were just written by `finish` above.
    let wav_bytes = unsafe { std::slice::from_raw_parts(out_data, out_len) }.to_vec();
    unsafe { mediaway_buffer_free(out_data, out_len) };
    unsafe { mediaway_wav_muxer_close(muxer) };

    assert_eq!(&wav_bytes[0..4], b"RIFF");
    assert_eq!(&wav_bytes[8..12], b"WAVE");

    let mut info = MediawayStreamInfo {
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
    let status = unsafe {
        mediaway_wav_parse(
            wav_bytes.as_ptr(),
            wav_bytes.len(),
            &raw mut info,
            &raw mut packet,
        )
    };
    assert_eq!(status, MediawayStatus::Ok, "parse");
    assert_eq!(info.codec, MediawayCodecKind::RawAudio);
    assert_eq!(info.sample_rate, 44_100);
    assert_eq!(info.channels, 2);
    assert_eq!(packet.payload_len, pcm.len());
    assert_eq!(packet.duration, 2, "2 PCM frames");
    // SAFETY: `packet.payload` was just written by `parse` above.
    let payload = unsafe { std::slice::from_raw_parts(packet.payload, packet.payload_len) };
    assert_eq!(payload, &pcm[..]);

    unsafe { mediaway_stream_info_free(&raw mut info) };
    unsafe { mediaway_packet_free(&raw mut packet) };
}

/// A muxer built with an explicit (non-PCM) format round-trips too.
#[test]
fn wav_float_format_mux_parse_round_trips_through_ffi() {
    let format = MediawayWaveFormat {
        sample_format: MediawayWavSampleFormat::Float,
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 32,
    };
    let muxer = unsafe { mediaway_wav_muxer_create_with_format(&raw const format) };
    assert!(!muxer.is_null());

    let pcm: [u8; 8] = [0xAA; 8]; // 2 frames of 4-byte (1ch x 32-bit) PCM
    let view = MediawayPacketView {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 0,
        is_keyframe: true,
        is_discard: false,
        payload: pcm.as_ptr(),
        payload_len: pcm.len(),
    };
    let status = unsafe { mediaway_wav_muxer_push_packet(muxer, &raw const view) };
    assert_eq!(status, MediawayStatus::Ok);

    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    let status = unsafe { mediaway_wav_muxer_finish(muxer, &raw mut out_data, &raw mut out_len) };
    assert_eq!(status, MediawayStatus::Ok);
    let wav_bytes = unsafe { std::slice::from_raw_parts(out_data, out_len) }.to_vec();
    unsafe { mediaway_buffer_free(out_data, out_len) };
    unsafe { mediaway_wav_muxer_close(muxer) };

    let mut info = MediawayStreamInfo {
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
    let status = unsafe {
        mediaway_wav_parse(
            wav_bytes.as_ptr(),
            wav_bytes.len(),
            &raw mut info,
            &raw mut packet,
        )
    };
    assert_eq!(status, MediawayStatus::Ok);
    assert_eq!(info.sample_rate, 48_000);
    assert_eq!(info.channels, 1);
    unsafe { mediaway_stream_info_free(&raw mut info) };
    unsafe { mediaway_packet_free(&raw mut packet) };
}

/// A second `finish()` call after the muxer's state was already consumed fails honestly.
#[test]
fn wav_muxer_second_finish_fails() {
    let muxer = mediaway_wav_muxer_create(44_100, 2, 16);
    assert!(!muxer.is_null());

    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    let status = unsafe { mediaway_wav_muxer_finish(muxer, &raw mut out_data, &raw mut out_len) };
    assert_eq!(status, MediawayStatus::Ok);
    unsafe { mediaway_buffer_free(out_data, out_len) };

    let mut out_data2 = std::ptr::null_mut();
    let mut out_len2 = 0usize;
    let status = unsafe { mediaway_wav_muxer_finish(muxer, &raw mut out_data2, &raw mut out_len2) };
    assert_eq!(status, MediawayStatus::InvalidState);

    unsafe { mediaway_wav_muxer_close(muxer) };
}

/// `parse` on non-WAV data fails honestly, not silently.
#[test]
fn wav_parse_rejects_non_riff_wave_data() {
    let data = b"not a wav file";
    let mut info = MediawayStreamInfo {
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
    let status =
        unsafe { mediaway_wav_parse(data.as_ptr(), data.len(), &raw mut info, &raw mut packet) };
    assert_eq!(status, MediawayStatus::InvalidData);
}
