//! Integration: real Opus encode -> decode round trip, entirely through
//! `mediaway-ffi`'s C ABI (`adr/pipeline/0006-audio-decode-c-abi.md`).
//!
//! Exercises both halves ADR-0006 adds: `mediaway_audio_encoder_open` with an Opus
//! config (previously AAC-only) feeding `mediaway_audio_decode_session_open`'s new
//! Opus decode session. Cross-platform and hermetic — `mediaway-sw`'s Opus backend
//! has no OS dependency and no real microphone is needed (synthetic sine PCM), so
//! unlike `audio_encode_smoke.rs` (AAC, Windows-only) this test has no `#![cfg(windows)]`
//! guard.

#![allow(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::panic,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::too_many_lines,
    reason = "integration test"
)]

use mediaway_ffi::pipeline::{
    MediawayAudioFrameView, MediawayDecodePacketView, MediawayPipelineCodecKind,
    MediawayPipelineStatus, MediawayRational, MediawaySampleFormat,
    mediaway_audio_decode_config_opus, mediaway_audio_decode_session_close,
    mediaway_audio_decode_session_flush, mediaway_audio_decode_session_open,
    mediaway_audio_decode_session_poll_frame, mediaway_audio_decode_session_push_packet,
    mediaway_audio_encode_config_opus, mediaway_audio_encode_session_close,
    mediaway_audio_encode_session_flush, mediaway_audio_encode_session_poll_packet,
    mediaway_audio_encode_session_push_pcm, mediaway_audio_encoder_open,
    mediaway_decoded_audio_frame_free, mediaway_pipeline_ffi_packet_free,
};

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 1;
/// 20 ms @ 48 kHz mono — a legal Opus frame duration.
const FRAME_SAMPLES: usize = 960;
const FRAME_COUNT: u32 = 50;
const TIME_BASE: MediawayRational = MediawayRational { num: 1, den: 50 };

/// `FRAME_COUNT` frames of a deterministic 440 Hz sine, F32 mono — no real
/// microphone needed, so the test is hermetic and fast.
fn sine_frames() -> Vec<Vec<u8>> {
    let mut frames = Vec::with_capacity(FRAME_COUNT as usize);
    for i in 0..FRAME_COUNT {
        let mut bytes = Vec::with_capacity(FRAME_SAMPLES * 4);
        for s in 0..FRAME_SAMPLES {
            let t = (i as usize * FRAME_SAMPLES + s) as f32 / SAMPLE_RATE as f32;
            let v = (t * 440.0 * std::f32::consts::TAU).sin();
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        frames.push(bytes);
    }
    frames
}

#[test]
fn opus_encode_decode_round_trips_through_ffi() {
    // ── encode: Opus config sugar (adr/pipeline/0006 §1) ──────────────────
    let enc_config = mediaway_audio_encode_config_opus(SAMPLE_RATE, CHANNELS, TIME_BASE);
    assert_eq!(enc_config.codec, MediawayPipelineCodecKind::Opus);
    assert_eq!(enc_config.sample_format, MediawaySampleFormat::F32);

    let mut enc_session = std::ptr::null_mut();
    let status =
        unsafe { mediaway_audio_encoder_open(&raw const enc_config, &raw mut enc_session) };
    assert_eq!(
        status,
        MediawayPipelineStatus::Ok,
        "Opus encoder open must succeed"
    );
    assert!(!enc_session.is_null());

    for (i, pcm) in sine_frames().into_iter().enumerate() {
        let frame = MediawayAudioFrameView {
            pts: i64::from(i as u32 * FRAME_SAMPLES as u32),
            duration: FRAME_SAMPLES as u64,
            sample_rate: SAMPLE_RATE,
            channels: CHANNELS,
            sample_format: MediawaySampleFormat::F32,
            data: pcm.as_ptr(),
            data_len: pcm.len(),
        };
        let status =
            unsafe { mediaway_audio_encode_session_push_pcm(enc_session, &raw const frame) };
        assert_eq!(status, MediawayPipelineStatus::Ok, "push_pcm frame {i}");
    }
    let status = unsafe { mediaway_audio_encode_session_flush(enc_session) };
    assert_eq!(status, MediawayPipelineStatus::Ok);

    let mut packets = Vec::new();
    loop {
        let mut packet = mediaway_ffi::pipeline::MediawayAudioPacket::default();
        let mut has = false;
        let status = unsafe {
            mediaway_audio_encode_session_poll_packet(enc_session, &raw mut packet, &raw mut has)
        };
        assert_eq!(status, MediawayPipelineStatus::Ok);
        if !has {
            break;
        }
        assert!(packet.payload_len > 0, "Opus packet must carry payload");
        // SAFETY: `payload`/`payload_len` are valid for the lifetime of `packet`.
        let payload =
            unsafe { std::slice::from_raw_parts(packet.payload, packet.payload_len) }.to_vec();
        packets.push((packet.pts, packet.duration, payload));
        unsafe { mediaway_pipeline_ffi_packet_free(&raw mut packet) };
    }
    assert_eq!(
        packets.len(),
        FRAME_COUNT as usize,
        "one Opus packet per pushed frame"
    );
    unsafe { mediaway_audio_encode_session_close(enc_session) };

    // ── decode: feed the real Opus packets back in ────────────────────────
    let dec_config = mediaway_audio_decode_config_opus(SAMPLE_RATE, CHANNELS, TIME_BASE);
    let mut dec_session = std::ptr::null_mut();
    let status =
        unsafe { mediaway_audio_decode_session_open(&raw const dec_config, &raw mut dec_session) };
    assert_eq!(
        status,
        MediawayPipelineStatus::Ok,
        "Opus decoder open must succeed"
    );
    assert!(!dec_session.is_null());

    for (pts, duration, payload) in &packets {
        let view = MediawayDecodePacketView {
            stream_id: 0,
            pts: *pts,
            dts: *pts,
            duration: *duration,
            is_keyframe: true,
            is_discard: false,
            payload: payload.as_ptr(),
            payload_len: payload.len(),
        };
        let status =
            unsafe { mediaway_audio_decode_session_push_packet(dec_session, &raw const view) };
        assert_eq!(status, MediawayPipelineStatus::Ok, "push_packet");
    }

    // A lost-frame hint (empty payload) must decode via PLC, not error.
    let plc_view = MediawayDecodePacketView {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: FRAME_SAMPLES as u64,
        is_keyframe: false,
        is_discard: false,
        payload: std::ptr::null(),
        payload_len: 0,
    };
    let status =
        unsafe { mediaway_audio_decode_session_push_packet(dec_session, &raw const plc_view) };
    assert_eq!(status, MediawayPipelineStatus::Ok, "PLC push_packet");

    let status = unsafe { mediaway_audio_decode_session_flush(dec_session) };
    assert_eq!(status, MediawayPipelineStatus::Ok);

    let mut decoded = 0;
    let mut total_energy = 0.0f64;
    loop {
        let mut frame = mediaway_ffi::pipeline::MediawayDecodedAudioFrame {
            pts: 0,
            duration: 0,
            sample_rate: 0,
            channels: 0,
            sample_format: MediawaySampleFormat::F32,
            data: std::ptr::null_mut(),
            data_len: 0,
        };
        let mut has = false;
        let status = unsafe {
            mediaway_audio_decode_session_poll_frame(dec_session, &raw mut frame, &raw mut has)
        };
        assert_eq!(status, MediawayPipelineStatus::Ok, "poll_frame");
        if !has {
            break;
        }
        assert_eq!(frame.sample_rate, SAMPLE_RATE);
        assert_eq!(frame.channels, CHANNELS);
        assert_eq!(frame.sample_format, MediawaySampleFormat::F32);
        assert_eq!(
            frame.data_len,
            FRAME_SAMPLES * 4,
            "one full 20ms frame of F32 mono PCM"
        );
        // SAFETY: `data`/`data_len` are valid for the lifetime of `frame`. Read via
        // `from_le_bytes` chunks rather than a `*const f32` cast — `data` is a
        // `Box<[u8]>`-derived allocation with no `f32` alignment guarantee.
        let bytes = unsafe { std::slice::from_raw_parts(frame.data, frame.data_len) };
        let energy: f64 = bytes
            .chunks_exact(4)
            .map(|b| {
                let v = f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
                v * v
            })
            .sum();
        total_energy += energy;
        decoded += 1;
        unsafe { mediaway_decoded_audio_frame_free(&raw mut frame) };
    }
    assert_eq!(
        decoded,
        FRAME_COUNT as usize + 1,
        "every pushed packet plus the PLC frame must decode"
    );
    assert!(
        total_energy > 0.0,
        "decoded PCM must carry real signal, not silence"
    );

    // ── teardown ────────────────────────────────────────────────────────────
    unsafe { mediaway_audio_decode_session_close(dec_session) };
}
