//! Integration: real AAC audio encoding through `mediaway-pipeline-ffi`'s C ABI
//! (`adr/0003-auto-audio-encode-c-abi.md`) — synthetic F32 sine PCM pushed
//! through `mediaway_audio_encode_session_push_pcm`, encoded packets polled
//! back, muxed into a playable audio-only fMP4 with the stream-info
//! `extra_data` (`AudioSpecificConfig`), then demuxed again to prove the AAC
//! samples round-trip.
//!
//! Mirrors `mediaway-pipeline/tests/screen_mic_av_smoke.rs`'s audio half, but
//! exercises the raw `#[unsafe(no_mangle)]` C surface a language binding calls
//! — the same entry points the C/Python/Node/C++ wrappers use.

#![cfg(windows)]
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

use mediaway_common::{Bytes, CodecKind, Rational, StreamInfo};
use mediaway_container::mp4::Demuxer;
use mediaway_pipeline_ffi::{
    MediawayAudioFrameView, MediawayPipelineCodecKind, MediawayPipelineStatus, MediawayRational,
    MediawaySampleFormat,
};

/// 1024 samples per frame @ 48 kHz ≈ 21 ms; ~96 frames ≈ 2.0 s of audio.
const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const FRAME_SAMPLES: usize = 1024;
const FRAME_COUNT: u32 = 96;

/// Build `FRAME_COUNT` frames of a deterministic 440 Hz sine, F32 interleaved
/// stereo — no real microphone needed, so the test is hermetic and fast.
fn sine_frames() -> Vec<Vec<u8>> {
    let mut frames = Vec::with_capacity(FRAME_COUNT as usize);
    for i in 0..FRAME_COUNT {
        let mut bytes = Vec::with_capacity(FRAME_SAMPLES * CHANNELS as usize * 4);
        for s in 0..FRAME_SAMPLES {
            let t = (i as usize * FRAME_SAMPLES + s) as f32 / SAMPLE_RATE as f32;
            let v = (t * 440.0 * std::f32::consts::TAU).sin();
            for _ in 0..CHANNELS {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
        }
        frames.push(bytes);
    }
    frames
}

/// Collect the `extra_data` bytes from an FFI `MediawayAudioStreamInfo`.
fn take_extra_data(info: &mut mediaway_pipeline_ffi::MediawayAudioStreamInfo) -> Bytes {
    let mut bytes = Vec::new();
    if !info.extra_data.is_null() {
        // SAFETY: `extra_data`/`extra_data_len` are valid for the lifetime of
        // `info` (owned by it until freed).
        bytes.extend_from_slice(unsafe {
            std::slice::from_raw_parts(info.extra_data, info.extra_data_len)
        });
    }
    Bytes::from(bytes)
}

#[test]
fn audio_encode_roundtrips_to_fmp4() {
    // ── open: sugar config, ABI v2 surface ─────────────────────────────────
    let config = mediaway_pipeline_ffi::mediaway_audio_encode_config_aac(
        SAMPLE_RATE,
        MediawayRational {
            num: 1,
            den: SAMPLE_RATE,
        },
    );
    assert_eq!(config.codec, MediawayPipelineCodecKind::Aac);
    assert_eq!(config.sample_format, MediawaySampleFormat::F32);

    let mut session: *mut mediaway_pipeline_ffi::AudioEncodeSessionHandle = std::ptr::null_mut();
    let status = unsafe {
        mediaway_pipeline_ffi::mediaway_audio_encoder_open(&raw const config, &raw mut session)
    };
    assert_eq!(
        status,
        MediawayPipelineStatus::Ok,
        "open must succeed on Windows"
    );
    assert!(!session.is_null());

    // ── push synthetic PCM ─────────────────────────────────────────────────
    // The AudioSpecificConfig only materializes on the encoder's output type
    // after the first input sample, so encode first, then query stream info
    // (the real C caller registers the muxer track with the ASC before muxing
    // packets — same order).
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
        let status = unsafe {
            mediaway_pipeline_ffi::mediaway_audio_encode_session_push_pcm(session, &raw const frame)
        };
        assert_eq!(status, MediawayPipelineStatus::Ok);
    }
    let status = unsafe { mediaway_pipeline_ffi::mediaway_audio_encode_session_flush(session) };
    assert_eq!(status, MediawayPipelineStatus::Ok);

    // ── stream info: codec config a muxer needs ────────────────────────────
    let mut info = mediaway_pipeline_ffi::MediawayAudioStreamInfo::default();
    let status = unsafe {
        mediaway_pipeline_ffi::mediaway_audio_encode_session_stream_info(session, &raw mut info)
    };
    assert_eq!(status, MediawayPipelineStatus::Ok);
    assert_eq!(info.codec, MediawayPipelineCodecKind::Aac);
    assert_eq!(info.sample_rate, SAMPLE_RATE);
    assert_eq!(info.channels, CHANNELS);
    assert!(info.extra_data_len > 0, "AAC AudioSpecificConfig expected");
    let extra_data = take_extra_data(&mut info);

    // ── poll encoded packets ───────────────────────────────────────────────
    let mut packets = Vec::new();
    loop {
        let mut packet = mediaway_pipeline_ffi::MediawayAudioPacket::default();
        let mut has = false;
        let status = unsafe {
            mediaway_pipeline_ffi::mediaway_audio_encode_session_poll_packet(
                session,
                &raw mut packet,
                &raw mut has,
            )
        };
        assert_eq!(status, MediawayPipelineStatus::Ok);
        if !has {
            break;
        }
        assert!(packet.payload_len > 0, "AAC packet must carry payload");
        // SAFETY: `payload`/`payload_len` are valid for the lifetime of `packet`.
        let payload =
            unsafe { std::slice::from_raw_parts(packet.payload, packet.payload_len) }.to_vec();
        packets.push((packet.pts, packet.is_keyframe, payload));
        // SAFETY: `packet` was filled by poll_packet above (function contract).
        unsafe { mediaway_pipeline_ffi::mediaway_pipeline_ffi_packet_free(&raw mut packet) };
    }
    assert!(!packets.is_empty(), "expected at least one AAC packet");

    // ── mux an audio-only fMP4 with the stream info's codec config ────────
    let mut open = mediaway_container::mp4::Muxer::with_fragment_batch(1);
    let ainfo = StreamInfo::Audio {
        id: 0,
        codec: CodecKind::Aac,
        time_base: Rational::new(1, SAMPLE_RATE),
        extra_data,
        sample_rate: SAMPLE_RATE,
        channels: CHANNELS,
    };
    open.add_track(ainfo).expect("audio track");
    let mut mux = open.begin();
    for (pts, is_keyframe, payload) in &packets {
        let p = mediaway_common::Packet {
            stream_id: 0,
            pts: *pts,
            dts: *pts,
            duration: FRAME_SAMPLES as u64,
            is_keyframe: *is_keyframe,
            is_discard: false,
            payload: Bytes::from(payload.clone()),
            // clone: the muxer copies the packet payload on push; keeping the
            // original keeps the post-mux loop below identical to what was polled.
        };
        mux.push_packet(&p).expect("mux audio packet");
    }
    mux.flush();
    let mut bytes = Vec::new();
    mux.poll_bytes(&mut bytes);
    assert_eq!(&bytes[4..8], b"ftyp", "fMP4 signature expected");
    assert!(
        bytes.len() > 1_000,
        "audio fMP4 implausibly small: {} bytes",
        bytes.len()
    );

    // ── demux: the AAC packets survive the round-trip ─────────────────────
    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    assert_eq!(demux.streams().len(), 1);
    match &demux.streams()[0] {
        // `sample_rate`/`channels` are deliberately 0 from the MP4 demux
        // (iso_bmff::Track doesn't carry them — see convert.rs::to_stream_info);
        // assert what the container contract does provide: codec + esds ASC.
        StreamInfo::Audio {
            codec, extra_data, ..
        } => {
            assert_eq!(*codec, CodecKind::Aac);
            assert_eq!(
                extra_data.as_ref(),
                [0x11, 0x90],
                "esds must round-trip the ASC"
            );
        }
        other => panic!("expected audio track, got {other:?}"),
    }
    let mut demuxed = 0;
    while let Some(p) = demux.poll_packet() {
        assert!(!p.payload.is_empty());
        assert_eq!(p.stream_id, 0);
        demuxed += 1;
    }
    assert_eq!(
        demuxed,
        packets.len(),
        "every encoded packet must demux back"
    );

    // ── teardown ───────────────────────────────────────────────────────────
    // SAFETY: `info` was filled by stream_info and not yet freed (function contract).
    unsafe { mediaway_pipeline_ffi::mediaway_pipeline_ffi_stream_info_free(&raw mut info) };
    // SAFETY: `session` was returned by open and not yet closed (function contract).
    unsafe { mediaway_pipeline_ffi::mediaway_audio_encode_session_close(session) };
}
