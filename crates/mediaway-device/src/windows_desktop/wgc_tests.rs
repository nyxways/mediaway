#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use super::resized_geometry;
use mediaway_common::VideoGeometry;

/// Full hardware-driven resize (actually resizing a captured window/monitor mid-session
/// and observing `Direct3D11CaptureFramePool::Recreate` take effect) is not practically
/// automatable in this suite — it needs a real WGC session plus a window an external
/// actor resizes on a timeline this test can't control. Instead, this exercises the pure
/// decision logic `poll_frame` uses to detect a size change, extracted so it is testable
/// without `WinRT` calls.

#[test]
fn resized_geometry_none_when_size_unchanged() {
    let current = VideoGeometry {
        width: 1920,
        height: 1080,
    };
    assert_eq!(resized_geometry(current, 1920, 1080), None);
}

#[test]
fn resized_geometry_some_when_width_changes() {
    let current = VideoGeometry {
        width: 1920,
        height: 1080,
    };
    assert_eq!(
        resized_geometry(current, 1280, 1080),
        Some(VideoGeometry {
            width: 1280,
            height: 1080,
        })
    );
}

#[test]
fn resized_geometry_some_when_height_changes() {
    let current = VideoGeometry {
        width: 1920,
        height: 1080,
    };
    assert_eq!(
        resized_geometry(current, 1920, 720),
        Some(VideoGeometry {
            width: 1920,
            height: 720,
        })
    );
}

#[test]
fn resized_geometry_some_on_first_frame_from_zero_geometry() {
    // `stream_info.geometry()` starts non-zero at `open()` in practice, but the closed/
    // uninitialized path uses a `0x0` placeholder — confirm that is always treated as a
    // mismatch (never accidentally suppresses a legitimate first Recreate).
    let current = VideoGeometry {
        width: 0,
        height: 0,
    };
    assert_eq!(
        resized_geometry(current, 800, 600),
        Some(VideoGeometry {
            width: 800,
            height: 600,
        })
    );
}
