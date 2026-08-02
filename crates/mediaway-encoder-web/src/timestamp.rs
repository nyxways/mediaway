//! Pure timestamp range validation, shared by the wasm encode path.
//!
//! `web-sys`'s `VideoFrameBufferInit::new_with_f64` accepts an `f64` timestamp (this crate's
//! public API unit for microseconds), but [`crate::encode_video_frames`] rejects values that
//! would not survive a later `i32` microsecond round trip (e.g. through
//! `mediaway-decoder-web`'s `EncodedVideoChunkInit`) so encode/decode failures stay
//! symmetric. No `web-sys`/`wasm-bindgen` types here so it compiles and is testable on the
//! host target too.

#![forbid(unsafe_code)]

/// Convert a microsecond timestamp to `i32` if it fits and is finite, else `None`.
#[allow(
    dead_code,
    reason = "only called from wasm32-only wasm.rs; exercised directly by host-side unit tests below"
)]
#[allow(
    clippy::redundant_pub_crate,
    reason = "explicit crate-only scope, even though the parent `timestamp` module is also private"
)]
#[must_use]
pub(crate) fn timestamp_us_to_i32(timestamp_us: f64) -> Option<i32> {
    if !timestamp_us.is_finite()
        || timestamp_us < f64::from(i32::MIN)
        || timestamp_us > f64::from(i32::MAX)
    {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "bounds-checked against i32::MIN/MAX just above the cast"
    )]
    Some(timestamp_us as i32)
}

#[cfg(test)]
#[path = "timestamp_tests.rs"]
mod tests;
