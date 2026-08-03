#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "unit tests may unwrap"
)]

use super::*;

#[test]
fn bgrx_maps_to_bgra8() {
    assert_eq!(
        map_spa_video_format(VideoFormat::BGRx),
        Some(PixelFormat::Bgra8)
    );
}

#[test]
fn rgba_maps_to_rgba8() {
    assert_eq!(
        map_spa_video_format(VideoFormat::RGBA),
        Some(PixelFormat::Rgba8)
    );
}

#[test]
fn rgbx_maps_to_rgba8() {
    assert_eq!(
        map_spa_video_format(VideoFormat::RGBx),
        Some(PixelFormat::Rgba8)
    );
}

#[test]
fn i420_maps_to_i420() {
    assert_eq!(
        map_spa_video_format(VideoFormat::I420),
        Some(PixelFormat::I420)
    );
}

#[test]
fn unoffered_format_maps_to_none() {
    // Never offered in the crate's `EnumFormat` choice list — must not be
    // silently coerced into a guessed `PixelFormat`.
    assert_eq!(map_spa_video_format(VideoFormat::YUY2), None);
    assert_eq!(map_spa_video_format(VideoFormat::RGB), None);
}
