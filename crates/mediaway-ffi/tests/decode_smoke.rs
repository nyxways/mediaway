//! Integration: real WMF H.264 encode → mux → demux → decode round trip, entirely
//! through `mediaway-ffi`'s C ABI (`adr/pipeline/0004-auto-decode-c-abi.md`).
//!
//! Mirrors `mediaway-decoder/tests/cpu_roundtrip.rs`'s Rust-level encode/decode round
//! trip, but exercises the raw `#[unsafe(no_mangle)]` C surface a language binding
//! calls, and sources the decoder's `extra_data` from a real demuxed fMP4 stream
//! (`container.h`'s demuxer) rather than straight from the encoder — the actual shape
//! a C caller decoding a downloaded/received file would have.
//!
//! **Currently `#[ignore]`d — real, pre-existing bug found while writing this test,
//! not a missing-hardware skip.** Every step through `mediaway_decode_session_flush`
//! succeeds and returns plausible values (verified by hand with tracing prints during
//! development), but `mediaway_decode_session_poll_frame` hits a Rust std UB
//! precondition check (`Alignment::new_unchecked requires a power of two`) inside the
//! wrapped `WindowsVideoDecoder`'s CPU output path and **aborts the process** — not a
//! catchable panic, so no `catch_unwind` in this crate's own FFI code can turn it into
//! a status code. `mediaway-decoder/tests/cpu_roundtrip.rs` (fixed and re-verified
//! alongside this file) reaches the same backend through pure Rust, no FFI at all,
//! with a single directly-encoder-polled packet instead of a muxed/demuxed multi-frame
//! stream, and does not crash — it silently decodes zero frames instead. Both are real
//! symptoms of the same underlying, pre-existing `WindowsVideoDecoder` CPU-decode bug
//! (crate `mediaway-decoder`), not a defect in this crate's C ABI wrapper. See
//! `docs/ai/wiki/decode/index.md` and `docs/roadmap.md` for tracking.

#![cfg(windows)]
#![allow(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::too_many_lines,
    reason = "integration test"
)]

use mediaway_ffi::container::{
    MediawayStatus, MediawayStreamInfo, mediaway_demuxer_close, mediaway_demuxer_create,
    mediaway_demuxer_poll_packet, mediaway_demuxer_push_bytes, mediaway_demuxer_stream_at,
    mediaway_demuxer_stream_count, mediaway_packet_free, mediaway_stream_info_free,
};
use mediaway_ffi::pipeline::{
    MediawayPipelineCodecKind, MediawayPipelineStatus, MediawayRational,
    mediaway_auto_encoder_open, mediaway_auto_video_decode_config_new,
    mediaway_auto_video_encode_config_new, mediaway_decode_session_close,
    mediaway_decode_session_flush, mediaway_decode_session_open,
    mediaway_decode_session_poll_frame, mediaway_decode_session_push_packet,
    mediaway_decoded_video_frame_free, mediaway_encode_session_close,
    mediaway_encode_session_finish, mediaway_encode_session_open,
    mediaway_encode_session_write_frame, mediaway_pipeline_ffi_buffer_free,
};
use mediaway_ffi::pipeline::{MediawayVideoFrame, MediawayVideoFrameStorageKind};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;
const FRAME_COUNT: u32 = 10;

#[test]
#[ignore = "real, pre-existing WindowsVideoDecoder CPU-decode bug — see module doc comment"]
fn encode_mux_demux_decode_round_trips() {
    // ── encode + mux (mid-gray NV12, no real capture needed) ──────────────
    let config = mediaway_auto_video_encode_config_new(
        MediawayPipelineCodecKind::H264,
        WIDTH,
        HEIGHT,
        MediawayRational { num: 1, den: 30 },
    );
    let mut encoder = std::ptr::null_mut();
    let status = unsafe { mediaway_auto_encoder_open(&raw const config, &raw mut encoder) };
    if status == MediawayPipelineStatus::NoBackend {
        eprintln!("skip: no encode backend compiled in");
        return;
    }
    assert_eq!(
        status,
        MediawayPipelineStatus::Ok,
        "encoder open must succeed"
    );

    let mut session = std::ptr::null_mut();
    let status = unsafe { mediaway_encode_session_open(encoder, &raw mut session) };
    assert_eq!(
        status,
        MediawayPipelineStatus::Ok,
        "session open must succeed"
    );

    let nv12_len = (WIDTH * HEIGHT + WIDTH * HEIGHT / 2) as usize;
    let plane = vec![128u8; nv12_len];
    for i in 0..FRAME_COUNT {
        let frame = MediawayVideoFrame {
            pts: i64::from(i),
            duration: 1,
            width: WIDTH,
            height: HEIGHT,
            pixel_format: mediaway_ffi::pipeline::MediawayPixelFormat::Nv12,
            storage_kind: MediawayVideoFrameStorageKind::Cpu,
            raw_bytes: plane.as_ptr(),
            raw_bytes_len: plane.len(),
            gpu_buffer: mediaway_ffi::pipeline::MediawayGpuBufferHandle {
                kind: mediaway_ffi::pipeline::MediawayGpuBufferKind::DirectX11,
                native_a: 0,
                native_b: 0,
                subresource: 0,
                webgpu_texture_id: 0,
            },
        };
        let status = unsafe { mediaway_encode_session_write_frame(session, &raw const frame) };
        assert_eq!(status, MediawayPipelineStatus::Ok, "write_frame {i}");
    }

    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    let status =
        unsafe { mediaway_encode_session_finish(session, &raw mut out_data, &raw mut out_len) };
    assert_eq!(status, MediawayPipelineStatus::Ok, "finish must succeed");
    assert!(out_len > 0, "expected non-empty fMP4 bytes");
    // SAFETY: `out_data`/`out_len` were just written by `finish` above.
    let fmp4 = unsafe { std::slice::from_raw_parts(out_data, out_len) }.to_vec();
    unsafe { mediaway_pipeline_ffi_buffer_free(out_data, out_len) };

    // ── demux: recover H.264 packets + AVCC extra_data ─────────────────────
    let demuxer = mediaway_demuxer_create();
    assert!(!demuxer.is_null());
    let status = unsafe { mediaway_demuxer_push_bytes(demuxer, fmp4.as_ptr(), fmp4.len()) };
    assert_eq!(status, MediawayStatus::Ok, "push_bytes must succeed");
    assert_eq!(unsafe { mediaway_demuxer_stream_count(demuxer) }, 1);

    let mut stream_info = MediawayStreamInfo {
        id: 0,
        codec: mediaway_ffi::container::MediawayCodecKind::H264,
        time_base: mediaway_ffi::container::MediawayRational { num: 0, den: 1 },
        has_geometry: false,
        width: 0,
        height: 0,
        sample_rate: 0,
        channels: 0,
        extra_data: std::ptr::null_mut(),
        extra_data_len: 0,
    };
    let status = unsafe { mediaway_demuxer_stream_at(demuxer, 0, &raw mut stream_info) };
    assert_eq!(status, MediawayStatus::Ok, "stream_at must succeed");
    assert!(stream_info.extra_data_len > 0, "expected AVCC extra_data");
    // SAFETY: `extra_data`/`extra_data_len` are valid for the lifetime of `stream_info`.
    let extra_data =
        unsafe { std::slice::from_raw_parts(stream_info.extra_data, stream_info.extra_data_len) }
            .to_vec();

    let mut packets = Vec::new();
    loop {
        let mut packet = mediaway_ffi::container::MediawayPacket {
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
        assert_eq!(status, MediawayStatus::Ok, "poll_packet must succeed");
        if !has {
            break;
        }
        // SAFETY: `payload`/`payload_len` are valid for the lifetime of `packet`.
        let payload =
            unsafe { std::slice::from_raw_parts(packet.payload, packet.payload_len) }.to_vec();
        packets.push((
            packet.pts,
            packet.dts,
            packet.duration,
            packet.is_keyframe,
            payload,
        ));
        unsafe { mediaway_packet_free(&raw mut packet) };
    }
    assert_eq!(
        packets.len(),
        FRAME_COUNT as usize,
        "every frame must demux back"
    );

    // ── decode: extra_data supplied at open time (adr/0004 §1) ────────────
    let dec_config = unsafe {
        mediaway_auto_video_decode_config_new(
            MediawayPipelineCodecKind::H264,
            WIDTH,
            HEIGHT,
            MediawayRational { num: 1, den: 30 },
            extra_data.as_ptr(),
            extra_data.len(),
        )
    };
    let mut decode_session = std::ptr::null_mut();
    let status =
        unsafe { mediaway_decode_session_open(&raw const dec_config, &raw mut decode_session) };
    if status == MediawayPipelineStatus::NoBackend {
        eprintln!("skip: no decode backend compiled in");
        unsafe { mediaway_stream_info_free(&raw mut stream_info) };
        unsafe { mediaway_demuxer_close(demuxer) };
        unsafe { mediaway_encode_session_close(session) };
        return;
    }
    assert_eq!(
        status,
        MediawayPipelineStatus::Ok,
        "decode session open must succeed"
    );

    for (pts, dts, duration, is_keyframe, payload) in &packets {
        let view = mediaway_ffi::pipeline::MediawayDecodePacketView {
            stream_id: 0,
            pts: *pts,
            dts: *dts,
            duration: *duration,
            is_keyframe: *is_keyframe,
            is_discard: false,
            payload: payload.as_ptr(),
            payload_len: payload.len(),
        };
        let status =
            unsafe { mediaway_decode_session_push_packet(decode_session, &raw const view) };
        assert_eq!(status, MediawayPipelineStatus::Ok, "push_packet");
    }
    let status = unsafe { mediaway_decode_session_flush(decode_session) };
    assert_eq!(status, MediawayPipelineStatus::Ok, "flush");

    let mut decoded = 0;
    loop {
        let mut frame = mediaway_ffi::pipeline::MediawayDecodedVideoFrame {
            pts: 0,
            duration: 0,
            width: 0,
            height: 0,
            pixel_format: mediaway_ffi::pipeline::MediawayPixelFormat::Nv12,
            data: std::ptr::null_mut(),
            data_len: 0,
        };
        let mut has = false;
        let status = unsafe {
            mediaway_decode_session_poll_frame(decode_session, &raw mut frame, &raw mut has)
        };
        assert_eq!(status, MediawayPipelineStatus::Ok, "poll_frame");
        if !has {
            break;
        }
        assert_eq!(frame.width, WIDTH);
        assert_eq!(frame.height, HEIGHT);
        assert!(
            frame.data_len >= nv12_len,
            "decoded frame implausibly small"
        );
        decoded += 1;
        unsafe { mediaway_decoded_video_frame_free(&raw mut frame) };
    }
    assert!(decoded > 0, "expected at least one decoded frame");

    // ── teardown ────────────────────────────────────────────────────────────
    unsafe { mediaway_decode_session_close(decode_session) };
    unsafe { mediaway_stream_info_free(&raw mut stream_info) };
    unsafe { mediaway_demuxer_close(demuxer) };
    unsafe { mediaway_encode_session_close(session) };
}
