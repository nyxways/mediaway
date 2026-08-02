//! Pure SPA `video/raw` format ↔ [`PixelFormat`] mapping.
//!
//! No `PipeWire` I/O — safe to unit test without a live portal/compositor
//! session (see crate ADR-0001 § Zero runtime verification this session).

#![forbid(unsafe_code)]

use mediaway_common::PixelFormat;
use pipewire::spa::param::video::VideoFormat;

/// Map a negotiated SPA `video/raw` pixel format to the closest [`PixelFormat`].
///
/// `BGRx` / `RGBx` carry an unused, undefined 4th byte (`x`, not a real alpha
/// channel) — mapping them to [`PixelFormat::Bgra8`] / [`PixelFormat::Rgba8`] is
/// an approximation. This is the same call the Windows DXGI backend already
/// makes for its BGRA desktop surface (see `mediaway-device-windows` ADR-0001);
/// consumers must not read that 4th byte as meaningful alpha for `BGRx`/`RGBx`
/// sources.
///
/// Returns `None` for any format this backend does not offer in its
/// `SPA_PARAM_EnumFormat` choice list (`screencast.rs`). That choice list and
/// this match **must be kept in sync by hand** — the format list is built by a
/// proc macro (`pw::spa::pod::object!`) that cannot consume this function's
/// match arms, so there is no compiler-enforced link between the two; see the
/// comment at the `object!` call site in `screencast.rs`.
#[must_use]
pub(crate) const fn map_spa_video_format(format: VideoFormat) -> Option<PixelFormat> {
    match format {
        VideoFormat::BGRx => Some(PixelFormat::Bgra8),
        VideoFormat::RGBx | VideoFormat::RGBA => Some(PixelFormat::Rgba8),
        VideoFormat::I420 => Some(PixelFormat::I420),
        _ => None,
    }
}

#[cfg(test)]
#[path = "format_tests.rs"]
mod tests;
