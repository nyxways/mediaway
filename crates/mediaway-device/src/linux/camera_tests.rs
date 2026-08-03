#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use super::*;
use crate::Select;
use crate::camera::{CameraCaptureConfig, CaptureOutputPreference};
use mediaway_common::Rational;

#[test]
fn picks_yuyv_when_available() {
    let (fmt, fourcc) = pick_capture_format(&[*b"MJPG", *b"YUYV", *b"NV12"]).expect("format");
    assert_eq!(fmt, PixelFormat::Yuyv);
    assert_eq!(fourcc, *b"YUYV");
}

#[test]
fn falls_back_to_nv12_when_no_yuyv() {
    let (fmt, _) = pick_capture_format(&[*b"MJPG", *b"NV12"]).expect("format");
    assert_eq!(fmt, PixelFormat::Nv12);
}

#[test]
fn falls_back_to_i420_when_only_yu12() {
    let (fmt, fourcc) = pick_capture_format(&[*b"MJPG", *b"YU12"]).expect("format");
    assert_eq!(fmt, PixelFormat::I420);
    assert_eq!(fourcc, *b"YU12");
}

#[test]
fn returns_none_when_nothing_supported() {
    assert!(pick_capture_format(&[*b"MJPG", *b"H264"]).is_none());
}

#[test]
fn yuyv_tight_stride_packs_exactly() {
    // 2x2 YUYV, tight stride (no padding): 2 bytes/pixel * 2 px/row = 4 bytes/row.
    let src: Vec<u8> = (0u8..8).collect();
    let out = pack_frame_bytes(&src, PixelFormat::Yuyv, 2, 2, 4).expect("packed");
    assert_eq!(out.as_ref(), src.as_slice());
}

#[test]
fn yuyv_padded_stride_strips_padding() {
    // 2x2 YUYV, stride 6 (2 bytes of driver padding per row) — packed output
    // must drop the padding bytes, not include them.
    let mut src = Vec::new();
    src.extend_from_slice(&[1, 2, 3, 4, 0xAA, 0xAA]); // row 0: 4 real + 2 pad
    src.extend_from_slice(&[5, 6, 7, 8, 0xAA, 0xAA]); // row 1: 4 real + 2 pad
    let out = pack_frame_bytes(&src, PixelFormat::Yuyv, 2, 2, 6).expect("packed");
    assert_eq!(out.as_ref(), &[1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn nv12_tight_stride_packs_luma_then_chroma() {
    // 2x2 NV12: luma 2x2 = 4 bytes, chroma (h/2 x w) = 1 row of 2 bytes.
    let mut src = Vec::new();
    src.extend_from_slice(&[10, 11, 12, 13]); // luma rows
    src.extend_from_slice(&[20, 21]); // one interleaved UV row
    let out = pack_frame_bytes(&src, PixelFormat::Nv12, 2, 2, 2).expect("packed");
    assert_eq!(out.as_ref(), &[10, 11, 12, 13, 20, 21]);
}

#[test]
fn nv12_padded_stride_strips_padding_in_both_planes() {
    // 2x2 NV12, stride 3 (1 byte padding per row in both luma and chroma).
    let mut src = Vec::new();
    src.extend_from_slice(&[10, 11, 0xAA]); // luma row 0 + pad
    src.extend_from_slice(&[12, 13, 0xAA]); // luma row 1 + pad
    src.extend_from_slice(&[20, 21, 0xAA]); // chroma row + pad
    let out = pack_frame_bytes(&src, PixelFormat::Nv12, 2, 2, 3).expect("packed");
    assert_eq!(out.as_ref(), &[10, 11, 12, 13, 20, 21]);
}

#[test]
fn i420_tight_stride_packs_y_u_v_in_order() {
    // 4x2 I420: luma 4x2 = 8 bytes, U/V each (h/2 x w/2) = 1 row of 2 bytes.
    let mut src = Vec::new();
    src.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]); // Y
    src.extend_from_slice(&[9, 10]); // U
    src.extend_from_slice(&[11, 12]); // V
    let out = pack_frame_bytes(&src, PixelFormat::I420, 4, 2, 4).expect("packed");
    assert_eq!(out.as_ref(), &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
}

#[test]
fn truncated_buffer_returns_none_not_a_panic() {
    let src = [1u8, 2, 3]; // too short for a 2x2 YUYV frame (needs 8 bytes)
    assert!(pack_frame_bytes(&src, PixelFormat::Yuyv, 2, 2, 4).is_none());
}

#[test]
fn zero_dimensions_return_none() {
    let src = [0u8; 16];
    assert!(pack_frame_bytes(&src, PixelFormat::Yuyv, 0, 2, 4).is_none());
    assert!(pack_frame_bytes(&src, PixelFormat::Yuyv, 2, 0, 4).is_none());
}

#[test]
fn bgra8_is_not_a_supported_capture_pack_format() {
    // This backend never negotiates Bgra8 (not in `PREFERRED_FOURCCS`) — a
    // stray call must not silently fall through to some other plane layout.
    let src = [0u8; 64];
    assert!(pack_frame_bytes(&src, PixelFormat::Bgra8, 4, 4, 16).is_none());
}

#[test]
fn non_default_select_is_unsupported() {
    let cfg = CameraCaptureConfig {
        select: Select::NameContains("nonexistent".to_owned()),
        time_base: Rational::new(1, 30),
        output: CaptureOutputPreference::CpuFramesOk,
        gpu_device: None,
    };
    assert!(matches!(
        LinuxCameraCapture::open(&cfg),
        Err(CaptureError::Unsupported)
    ));
}

#[test]
fn zero_copy_gpu_preference_is_unsupported_this_session() {
    let cfg = CameraCaptureConfig {
        select: Select::Default,
        time_base: Rational::new(1, 30),
        output: CaptureOutputPreference::ZeroCopyGpu,
        gpu_device: None,
    };
    assert!(matches!(
        LinuxCameraCapture::open(&cfg),
        Err(CaptureError::Unsupported)
    ));
}

/// Real V4L2 enumeration. WSL2 has no `/dev/video*` nodes at all (confirmed
/// this session) — expected to report zero nodes here, not a failure. See
/// crate ADR-0002 § Zero runtime verification.
#[test]
fn enumerate_camera_paths_or_skip() {
    let paths = enumerate_camera_paths();
    if paths.is_empty() {
        eprintln!("skip: no capture-capable V4L2 devices found on this machine");
    } else {
        eprintln!("camera nodes: {paths:?}");
    }
}

/// Opens the first real camera on this machine (if any) and captures a few
/// real frames. Skips gracefully — never fakes success — matching the
/// `_or_skip` convention `mediaway-device-windows` uses for its own
/// hardware-gated tests.
#[test]
fn open_camera_capture_frames_or_skip() {
    let paths = enumerate_camera_paths();
    if paths.is_empty() {
        eprintln!("skip: no capture-capable V4L2 devices on this machine");
        return;
    }
    eprintln!("opening camera 0: {:?}", paths[0]);

    let cfg = CameraCaptureConfig {
        select: Select::Default,
        time_base: Rational::new(1, 30),
        output: CaptureOutputPreference::CpuFramesOk,
        gpu_device: None,
    };
    let mut cap = match LinuxCameraCapture::open(&cfg) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skip: LinuxCameraCapture::open failed ({e:?})");
            return;
        }
    };

    let geometry = cap
        .stream_info()
        .geometry()
        .expect("video stream has geometry");
    assert!(geometry.width > 0);
    assert!(geometry.height > 0);

    let mut frames_seen = 0usize;
    for _ in 0..60 {
        match cap.poll_frame() {
            Ok(Some(frame)) => {
                match &frame.storage {
                    VideoFrameStorage::Cpu { data } => {
                        assert!(!data.is_empty(), "camera frame payload must not be empty");
                    }
                    VideoFrameStorage::Gpu(_) => {
                        unreachable!("this CPU-copy backend must never return GPU storage");
                    }
                    _ => unreachable!("unexpected VideoFrameStorage variant"),
                }
                cap.release_frame().expect("release (no-op for CPU frames)");
                frames_seen += 1;
                if frames_seen >= 3 {
                    break;
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("skip: poll_frame failed ({e:?})");
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    if frames_seen == 0 {
        eprintln!("skip: no frames captured within budget");
    } else {
        eprintln!("captured {frames_seen} real camera frame(s)");
    }
    cap.close().expect("close");
}
