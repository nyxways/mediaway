//! Integration: real AAC encode -> decode round trip through `mediaway-ffi`'s C ABI
//! (`adr/pipeline/0007-stream-bytes-aac-decode-support-probe.md` §2).
//!
//! Encodes synthetic PCM with `mediaway_audio_encoder_open` (Windows WMF AAC), takes the
//! `AudioSpecificConfig` from its stream info, and feeds the packets to
//! `mediaway_audio_decode_session_open` with `mediaway_audio_decode_config_aac`. The AAC
//! encoder is Windows-only, so this test is too; the Apple decode arm is compile-checked only.

#![cfg(all(windows, feature = "pipeline"))]
#![allow(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::panic,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::cast_possible_wrap,
    clippy::missing_const_for_fn,
    clippy::suboptimal_flops,
    reason = "integration test"
)]

use mediaway_ffi::pipeline::{
    MediawayAudioFrameView, MediawayDecodePacketView, MediawayDecodedAudioFrame,
    MediawayPipelineStatus, MediawayRational, MediawaySampleFormat,
    mediaway_audio_decode_config_aac, mediaway_audio_decode_session_close,
    mediaway_audio_decode_session_flush, mediaway_audio_decode_session_open,
    mediaway_audio_decode_session_poll_frame, mediaway_audio_decode_session_push_packet,
    mediaway_audio_encode_config_aac, mediaway_audio_encode_session_flush,
    mediaway_audio_encode_session_poll_packet, mediaway_audio_encode_session_push_pcm,
    mediaway_audio_encode_session_stream_info, mediaway_audio_encoder_open,
    mediaway_decoded_audio_frame_free, mediaway_pipeline_ffi_packet_free,
    mediaway_pipeline_ffi_stream_info_free,
};

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const FRAME_SAMPLES: usize = 1024;
const FRAME_COUNT: u32 = 48;
const TIME_BASE: MediawayRational = MediawayRational {
    num: 1,
    den: SAMPLE_RATE,
};

/// Encodes a 440 Hz stereo sine and returns `(AudioSpecificConfig, packets)`.
fn encode_aac() -> (Vec<u8>, Vec<(i64, u64, Vec<u8>)>) {
    let config = mediaway_audio_encode_config_aac(SAMPLE_RATE, TIME_BASE);
    let mut session = std::ptr::null_mut();
    let status = unsafe { mediaway_audio_encoder_open(&raw const config, &raw mut session) };
    assert_eq!(status, MediawayPipelineStatus::Ok, "AAC encoder open");

    for i in 0..FRAME_COUNT {
        let mut pcm = Vec::with_capacity(FRAME_SAMPLES * CHANNELS as usize * 4);
        for s in 0..FRAME_SAMPLES {
            let t = (i as usize * FRAME_SAMPLES + s) as f32 / SAMPLE_RATE as f32;
            let v = (t * 440.0 * std::f32::consts::TAU).sin();
            for _ in 0..CHANNELS {
                pcm.extend_from_slice(&v.to_le_bytes());
            }
        }
        let frame = MediawayAudioFrameView {
            pts: i64::from(i) * FRAME_SAMPLES as i64,
            duration: FRAME_SAMPLES as u64,
            sample_rate: SAMPLE_RATE,
            channels: CHANNELS,
            sample_format: MediawaySampleFormat::F32,
            data: pcm.as_ptr(),
            data_len: pcm.len(),
        };
        let status = unsafe { mediaway_audio_encode_session_push_pcm(session, &raw const frame) };
        assert_eq!(status, MediawayPipelineStatus::Ok, "push_pcm {i}");
    }
    let status = unsafe { mediaway_audio_encode_session_flush(session) };
    assert_eq!(status, MediawayPipelineStatus::Ok);

    // The ASC only exists after the first pushed frame.
    let mut info = mediaway_ffi::pipeline::MediawayAudioStreamInfo::default();
    let status = unsafe { mediaway_audio_encode_session_stream_info(session, &raw mut info) };
    assert_eq!(status, MediawayPipelineStatus::Ok);
    assert!(info.extra_data_len > 0, "AAC AudioSpecificConfig expected");
    // SAFETY: valid for the lifetime of `info` (owned until freed below).
    let asc = unsafe { std::slice::from_raw_parts(info.extra_data, info.extra_data_len) }.to_vec();
    unsafe { mediaway_pipeline_ffi_stream_info_free(&raw mut info) };

    let mut packets = Vec::new();
    loop {
        let mut packet = mediaway_ffi::pipeline::MediawayAudioPacket::default();
        let mut has = false;
        let status = unsafe {
            mediaway_audio_encode_session_poll_packet(session, &raw mut packet, &raw mut has)
        };
        assert_eq!(status, MediawayPipelineStatus::Ok);
        if !has {
            break;
        }
        // SAFETY: valid for the lifetime of `packet`.
        let payload =
            unsafe { std::slice::from_raw_parts(packet.payload, packet.payload_len) }.to_vec();
        packets.push((packet.pts, packet.duration, payload));
        unsafe { mediaway_pipeline_ffi_packet_free(&raw mut packet) };
    }
    unsafe { mediaway_ffi::pipeline::mediaway_audio_encode_session_close(session) };
    assert!(!packets.is_empty(), "expected AAC packets");
    (asc, packets)
}

fn view(pts: i64, duration: u64, payload: &[u8]) -> MediawayDecodePacketView {
    MediawayDecodePacketView {
        stream_id: 0,
        pts,
        dts: pts,
        duration,
        is_keyframe: true,
        is_discard: false,
        payload: if payload.is_empty() {
            std::ptr::null()
        } else {
            payload.as_ptr()
        },
        payload_len: payload.len(),
    }
}

#[test]
fn aac_encode_decode_round_trips_through_ffi() {
    let (asc, packets) = encode_aac();

    let config =
        mediaway_audio_decode_config_aac(SAMPLE_RATE, CHANNELS, TIME_BASE, asc.as_ptr(), asc.len());
    let mut session = std::ptr::null_mut();
    let status = unsafe { mediaway_audio_decode_session_open(&raw const config, &raw mut session) };
    assert_eq!(status, MediawayPipelineStatus::Ok, "AAC decoder open");

    for (pts, duration, payload) in &packets {
        let v = view(*pts, *duration, payload);
        let status = unsafe { mediaway_audio_decode_session_push_packet(session, &raw const v) };
        assert_eq!(status, MediawayPipelineStatus::Ok, "push_packet");
    }
    let status = unsafe { mediaway_audio_decode_session_flush(session) };
    assert_eq!(status, MediawayPipelineStatus::Ok);

    let mut samples = 0usize;
    let mut energy = 0.0f64;
    loop {
        let mut frame = MediawayDecodedAudioFrame {
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
            mediaway_audio_decode_session_poll_frame(session, &raw mut frame, &raw mut has)
        };
        assert_eq!(status, MediawayPipelineStatus::Ok, "poll_frame");
        if !has {
            break;
        }
        assert_eq!(frame.sample_format, MediawaySampleFormat::F32);
        assert_eq!(frame.channels, CHANNELS);
        // SAFETY: valid for the lifetime of `frame`.
        let bytes = unsafe { std::slice::from_raw_parts(frame.data, frame.data_len) };
        for chunk in bytes.as_chunks::<4>().0 {
            let v = f64::from(f32::from_le_bytes(*chunk));
            energy += v * v;
        }
        samples += frame.data_len / 4 / usize::from(CHANNELS);
        unsafe { mediaway_decoded_audio_frame_free(&raw mut frame) };
    }
    unsafe { mediaway_audio_decode_session_close(session) };

    let pushed = packets.len() * FRAME_SAMPLES;
    assert!(
        samples >= pushed.saturating_sub(FRAME_SAMPLES) && samples <= pushed + FRAME_SAMPLES,
        "decoded {samples} samples per channel for {pushed} pushed"
    );
    // A real signal, not silence: a unit sine has mean square 0.5.
    let mean_square = energy / (samples * usize::from(CHANNELS)) as f64;
    assert!(
        mean_square > 0.1,
        "decoded audio is near-silent (mean square {mean_square})"
    );
    eprintln!(
        "aac_decode: {} packets -> {samples} samples/ch, mean square {mean_square:.3}",
        packets.len()
    );
}

#[test]
fn aac_open_and_push_rules() {
    // Empty AudioSpecificConfig: a config mistake, not a missing capability.
    let no_asc =
        mediaway_audio_decode_config_aac(SAMPLE_RATE, CHANNELS, TIME_BASE, std::ptr::null(), 0);
    let mut session = std::ptr::null_mut();
    let status = unsafe { mediaway_audio_decode_session_open(&raw const no_asc, &raw mut session) };
    assert_eq!(status, MediawayPipelineStatus::InvalidInput);
    assert!(session.is_null());

    // A non-null pointer with zero length is the same empty ASC.
    let byte = 0x12u8;
    let zero_len =
        mediaway_audio_decode_config_aac(SAMPLE_RATE, CHANNELS, TIME_BASE, &raw const byte, 0);
    let status =
        unsafe { mediaway_audio_decode_session_open(&raw const zero_len, &raw mut session) };
    assert_eq!(status, MediawayPipelineStatus::InvalidInput);

    // A real ASC opens; an empty packet is refused (Opus's PLC hint means nothing for AAC).
    let asc = [0x11u8, 0x90]; // AAC-LC, 48 kHz, stereo
    let ok =
        mediaway_audio_decode_config_aac(SAMPLE_RATE, CHANNELS, TIME_BASE, asc.as_ptr(), asc.len());
    let status = unsafe { mediaway_audio_decode_session_open(&raw const ok, &raw mut session) };
    assert_eq!(status, MediawayPipelineStatus::Ok);
    let empty = view(0, 1024, &[]);
    let status = unsafe { mediaway_audio_decode_session_push_packet(session, &raw const empty) };
    assert_eq!(status, MediawayPipelineStatus::InvalidInput);
    unsafe { mediaway_audio_decode_session_close(session) };
}
