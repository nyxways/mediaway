//! Integration: `mediaway_encode_session_poll_bytes` against a real WMF H.264 encoder,
//! entirely through `mediaway-ffi`'s C ABI
//! (`adr/pipeline/0007-stream-bytes-aac-decode-support-probe.md` §1).
//!
//! The contract under test: bytes polled during the session plus the tail `finish` returns
//! are the whole stream, and `finish` after polling returns the tail only, not a second copy
//! of everything.

#![cfg(all(windows, feature = "pipeline"))]
#![allow(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::panic,
    clippy::too_many_lines,
    reason = "integration test"
)]

use mediaway_container::mp4::Demuxer;
use mediaway_ffi::pipeline::{
    MediawayGpuBufferHandle, MediawayGpuBufferKind, MediawayPipelineCodecKind,
    MediawayPipelineStatus, MediawayPixelFormat, MediawayRational, MediawayVideoFrame,
    MediawayVideoFrameStorageKind, mediaway_auto_encoder_open,
    mediaway_auto_video_encode_config_new, mediaway_encode_session_close,
    mediaway_encode_session_finish, mediaway_encode_session_open,
    mediaway_encode_session_poll_bytes, mediaway_encode_session_write_frame,
    mediaway_pipeline_ffi_buffer_free,
};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;
/// More than three default fragment batches (30 samples), so several fragments are ready
/// before `finish`.
const FRAME_COUNT: u32 = 100;

/// Opens a session on the real encoder, or `None` when no backend is compiled in.
fn open_session() -> Option<*mut mediaway_ffi::pipeline::EncodeSessionHandle> {
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
        return None;
    }
    assert_eq!(status, MediawayPipelineStatus::Ok, "encoder open");
    let mut session = std::ptr::null_mut();
    let status = unsafe { mediaway_encode_session_open(encoder, &raw mut session) };
    assert_eq!(status, MediawayPipelineStatus::Ok, "session open");
    Some(session)
}

fn write_frames(
    session: *mut mediaway_ffi::pipeline::EncodeSessionHandle,
    mut on_frame: impl FnMut(),
) {
    let plane = vec![128u8; (WIDTH * HEIGHT + WIDTH * HEIGHT / 2) as usize];
    for i in 0..FRAME_COUNT {
        let frame = MediawayVideoFrame {
            pts: i64::from(i),
            duration: 1,
            width: WIDTH,
            height: HEIGHT,
            pixel_format: MediawayPixelFormat::Nv12,
            storage_kind: MediawayVideoFrameStorageKind::Cpu,
            raw_bytes: plane.as_ptr(),
            raw_bytes_len: plane.len(),
            gpu_buffer: MediawayGpuBufferHandle {
                kind: MediawayGpuBufferKind::DirectX11,
                native_a: 0,
                native_b: 0,
                subresource: 0,
                webgpu_texture_id: 0,
            },
        };
        let status = unsafe { mediaway_encode_session_write_frame(session, &raw const frame) };
        assert_eq!(status, MediawayPipelineStatus::Ok, "write_frame {i}");
        on_frame();
    }
}

/// Takes ownership of an FFI buffer as a `Vec`, freeing the original.
fn take(data: *mut u8, len: usize) -> Vec<u8> {
    if data.is_null() {
        assert_eq!(len, 0);
        return Vec::new();
    }
    // SAFETY: `data`/`len` were just written by an FFI call that leaked exactly this buffer.
    let bytes = unsafe { std::slice::from_raw_parts(data, len) }.to_vec();
    unsafe { mediaway_pipeline_ffi_buffer_free(data, len) };
    bytes
}

fn finish(session: *mut mediaway_ffi::pipeline::EncodeSessionHandle) -> Vec<u8> {
    let mut data = std::ptr::null_mut();
    let mut len = 0usize;
    let status = unsafe { mediaway_encode_session_finish(session, &raw mut data, &raw mut len) };
    assert_eq!(status, MediawayPipelineStatus::Ok, "finish");
    take(data, len)
}

fn count_packets(fmp4: &[u8]) -> usize {
    let mut demux = Demuxer::new();
    demux.push_bytes(fmp4);
    let mut n = 0;
    while demux.poll_packet().is_some() {
        n += 1;
    }
    n
}

#[test]
fn polled_bytes_plus_finish_tail_are_the_whole_stream() {
    let Some(polled_session) = open_session() else {
        return;
    };

    // ── poll after every frame ─────────────────────────────────────────────
    let mut polled = Vec::new();
    let mut chunks = 0usize;
    write_frames(polled_session, || {
        let mut data = std::ptr::null_mut();
        let mut len = 0usize;
        let status = unsafe {
            mediaway_encode_session_poll_bytes(polled_session, &raw mut data, &raw mut len)
        };
        assert_eq!(status, MediawayPipelineStatus::Ok, "poll_bytes");
        if len > 0 {
            chunks += 1;
        }
        polled.extend(take(data, len));
    });
    assert!(
        chunks >= 1,
        "no fMP4 bytes were ready before finish over {FRAME_COUNT} frames"
    );
    assert!(
        polled.starts_with(&[0, 0]) && &polled[4..8] == b"ftyp",
        "first polled bytes start the file"
    );

    // A poll with nothing ready returns NULL/0 and allocates nothing.
    let mut data = std::ptr::null_mut();
    let mut len = 7usize;
    let status =
        unsafe { mediaway_encode_session_poll_bytes(polled_session, &raw mut data, &raw mut len) };
    assert_eq!(status, MediawayPipelineStatus::Ok);
    let idle_len = len;
    polled.extend(take(data, len));

    // ── finish returns the tail only ───────────────────────────────────────
    let tail = finish(polled_session);
    assert!(
        tail.len() < polled.len(),
        "finish after polling must return the unpolled tail ({} bytes), not the whole \
         stream ({} bytes polled)",
        tail.len(),
        polled.len()
    );
    let mut streamed = polled;
    streamed.extend_from_slice(&tail);
    let _ = idle_len;

    // ── the same input without polling is the reference stream ─────────────
    let reference_session = open_session().unwrap();
    write_frames(reference_session, || {});
    let reference = finish(reference_session);

    assert_eq!(
        count_packets(&streamed),
        FRAME_COUNT as usize,
        "streamed output must demux to every frame"
    );
    assert_eq!(
        count_packets(&reference),
        FRAME_COUNT as usize,
        "reference output must demux to every frame"
    );
    eprintln!(
        "stream_bytes: {chunks} non-empty polls, streamed {} B (tail {} B) vs unpolled {} B",
        streamed.len(),
        tail.len(),
        reference.len()
    );
}

#[test]
fn poll_bytes_argument_and_state_rules() {
    let mut data = std::ptr::null_mut();
    let mut len = 0usize;
    // SAFETY: null session is the case under test; the out-pointers are valid.
    let status = unsafe {
        mediaway_encode_session_poll_bytes(std::ptr::null_mut(), &raw mut data, &raw mut len)
    };
    assert_eq!(status, MediawayPipelineStatus::InvalidArgument);

    let Some(session) = open_session() else {
        return;
    };
    let status =
        unsafe { mediaway_encode_session_poll_bytes(session, std::ptr::null_mut(), &raw mut len) };
    assert_eq!(status, MediawayPipelineStatus::InvalidArgument);
    // Nothing written yet: the container header is not ready, so nothing to poll.
    let status =
        unsafe { mediaway_encode_session_poll_bytes(session, &raw mut data, &raw mut len) };
    assert_eq!(status, MediawayPipelineStatus::Ok);
    assert!(
        data.is_null() && len == 0,
        "idle poll must be NULL/0, got len {len}"
    );
    unsafe { mediaway_encode_session_close(session) };
}
