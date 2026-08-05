//! Integration: real USB camera capture pushed straight into a real WMF H.264
//! encode session through the capture-to-encode bridge
//! (`adr/pipeline/0005-capture-encode-bridge-c-abi.md`) — no
//! `mediaway_camera_frame_t` ever touched by this test, unlike
//! `bindings/c/examples/device/camera_record.c`'s hand-rolled poll+copy+write
//! loop. Skips (does not fail) when no camera is attached.

#![cfg(all(windows, feature = "pipeline", feature = "camera"))]
#![allow(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::panic,
    reason = "integration test"
)]

use std::time::{Duration, Instant};

use mediaway_ffi::device::{
    MediawayDeviceStatus, MediawayRational as MediawayDeviceRational,
    mediaway_camera_capture_close, mediaway_camera_capture_config_default,
    mediaway_camera_capture_geometry, mediaway_camera_capture_open,
};
use mediaway_ffi::pipeline::{
    MediawayPipelineCodecKind, MediawayPipelineStatus, MediawayRational,
    mediaway_auto_encoder_open, mediaway_auto_video_encode_config_new,
    mediaway_encode_session_finish, mediaway_encode_session_open,
    mediaway_encode_session_write_frame_from_camera_capture, mediaway_pipeline_ffi_buffer_free,
};

const TARGET_FRAMES: u32 = 5;
const POLL_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(20);

#[test]
fn camera_capture_bridges_straight_into_encode_session() {
    let cam_config =
        mediaway_camera_capture_config_default(0, MediawayDeviceRational { num: 1, den: 30 });
    let mut camera = std::ptr::null_mut();
    let status = unsafe { mediaway_camera_capture_open(&raw const cam_config, &raw mut camera) };
    if status == MediawayDeviceStatus::NoBackend {
        eprintln!("skip: no camera capture backend compiled in");
        return;
    }
    if status != MediawayDeviceStatus::Ok {
        eprintln!("skip: camera open failed ({status:?}) — no camera attached?");
        return;
    }

    let mut width = 0u32;
    let mut height = 0u32;
    let status =
        unsafe { mediaway_camera_capture_geometry(camera, &raw mut width, &raw mut height) };
    assert_eq!(status, MediawayDeviceStatus::Ok, "geometry must succeed");
    assert!(width > 0 && height > 0, "expected real camera geometry");

    let enc_config = mediaway_auto_video_encode_config_new(
        MediawayPipelineCodecKind::H264,
        width,
        height,
        MediawayRational { num: 1, den: 30 },
    );
    let mut encoder = std::ptr::null_mut();
    let status = unsafe { mediaway_auto_encoder_open(&raw const enc_config, &raw mut encoder) };
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

    let mut written = 0u32;
    let deadline = Instant::now() + POLL_TIMEOUT;
    while written < TARGET_FRAMES && Instant::now() < deadline {
        let mut wrote = false;
        let status = unsafe {
            mediaway_encode_session_write_frame_from_camera_capture(session, camera, &raw mut wrote)
        };
        assert_eq!(
            status,
            MediawayPipelineStatus::Ok,
            "bridge write must succeed"
        );
        if wrote {
            written += 1;
        } else {
            std::thread::sleep(POLL_INTERVAL);
        }
    }
    assert!(
        written == TARGET_FRAMES,
        "expected {TARGET_FRAMES} real camera frames, got {written}"
    );

    let mut out_data = std::ptr::null_mut();
    let mut out_len = 0usize;
    // `finish` takes ownership of `session` unconditionally — do not close it
    // afterward (adr/pipeline/0001-auto-encode-c-abi.md's documented contract).
    let status =
        unsafe { mediaway_encode_session_finish(session, &raw mut out_data, &raw mut out_len) };
    assert_eq!(status, MediawayPipelineStatus::Ok, "finish must succeed");
    assert!(out_len > 8, "expected non-empty fMP4 bytes");
    // SAFETY: `out_data`/`out_len` were just written by `finish` above.
    let fmp4 = unsafe { std::slice::from_raw_parts(out_data, out_len) };
    assert_eq!(&fmp4[4..8], b"ftyp", "fMP4 signature expected");
    unsafe { mediaway_pipeline_ffi_buffer_free(out_data, out_len) };

    unsafe { mediaway_camera_capture_close(camera) };
}
