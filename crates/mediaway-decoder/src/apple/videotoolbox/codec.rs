//! Pure helpers: `CMTime`↔[`Rational`] tick math, NV12 plane-copy byte math, and AVCC
//! parameter-set validation. Deliberately **no `objc2-*` dependency** — unlike [`super::video`],
//! this module has zero real `VideoToolbox` calls, so (like `linux::vaapi::nv12`) it builds and
//! unit-tests on any host, including this crate's non-Apple CI/dev hosts. [`super::video`]
//! converts between real `CMTime`/`CVPixelBuffer` values and this module's plain-integer /
//! byte-slice inputs.
//!
//! See [ADR-0001](../../../adr/apple/0001-videotoolbox-h264-cpu-out.md) § Timestamps, §
//! Callback / output-collection design, and § Byte framing for the math this module implements.

use iso_bmff::bitstream::avc::AvcDecoderConfig;
use iso_bmff::bitstream::hevc::HevcDecoderConfig;
use mediaway_common::{Bytes, CodecKind, Rational};

use crate::DecodeError;

/// Whether this crate's Apple decode path accepts `codec` — H.264/HEVC (NAL-based, in-band
/// parameter sets) and VP9/AV1 (raw, container-supplied config record) — see ADR-0002 § Scope.
#[must_use]
pub(super) const fn is_supported_video_codec(codec: CodecKind) -> bool {
    matches!(
        codec,
        CodecKind::H264 | CodecKind::Hevc | CodecKind::Vp9 | CodecKind::Av1
    )
}

/// Whether `codec` needs a container-supplied config record (`vpcC`/`av1C`) in
/// [`crate::VideoDecoderConfig::extra_data`] **at `open()`** — VP9/AV1 have no per-frame
/// parameter-set NAL this backend can discover from the bitstream itself (`VideoToolbox`'s only
/// construction path for either is the generic `CMVideoFormatDescriptionCreate` plus an
/// extension atom — see `format_desc::create_raw`'s doc comment), unlike H.264/HEVC's in-band
/// VPS/SPS/PPS.
#[must_use]
pub(super) const fn requires_extra_data_at_open(codec: CodecKind) -> bool {
    matches!(codec, CodecKind::Vp9 | CodecKind::Av1)
}

/// Container-config atom key `VideoToolbox`'s `SampleDescriptionExtensionAtoms` extension expects
/// for a raw (non-NAL) codec — `None` for anything else, which doesn't use this construction
/// path (see [`requires_extra_data_at_open`]).
#[must_use]
pub(super) const fn raw_atom_key(codec: CodecKind) -> Option<&'static str> {
    match codec {
        CodecKind::Vp9 => Some("vpcC"),
        CodecKind::Av1 => Some("av1C"),
        _ => None,
    }
}

/// Validate `config` against this backend's H.264 scope: exactly one SPS + one PPS, 4-byte
/// AVCC length-prefix size only (ADR-0001 § Byte framing / § Session lifecycle). A stream that
/// doesn't fit is `DecodeError::Unsupported`, never silently truncated to the first SPS/PPS.
///
/// # Errors
///
/// Returns [`DecodeError::Unsupported`] when `config` has more than one SPS/PPS or a NAL length
/// size other than 4 bytes.
pub(super) const fn validate_parameter_sets(config: &AvcDecoderConfig) -> Result<(), DecodeError> {
    if config.nal_length_size != 4 {
        return Err(DecodeError::Unsupported);
    }
    if config.sps.len() != 1 || config.pps.len() != 1 {
        return Err(DecodeError::Unsupported);
    }
    Ok(())
}

/// Validate `config` against this backend's HEVC scope: exactly one VPS + one SPS + one PPS,
/// 4-byte `hvcC` length-prefix size only — the HEVC analog of [`validate_parameter_sets`].
///
/// # Errors
///
/// Returns [`DecodeError::Unsupported`] when `config` has more than one VPS/SPS/PPS or a NAL
/// length size other than 4 bytes.
pub(super) const fn validate_hevc_parameter_sets(
    config: &HevcDecoderConfig,
) -> Result<(), DecodeError> {
    if config.nal_length_size != 4 {
        return Err(DecodeError::Unsupported);
    }
    if config.vps.len() != 1 || config.sps.len() != 1 || config.pps.len() != 1 {
        return Err(DecodeError::Unsupported);
    }
    Ok(())
}

/// Build the `(value, timescale)` pair for a `CMTime` from `ticks` (in `time_base` units):
/// `value = ticks * time_base.num`, `timescale = time_base.den` — algebraically exact since
/// `CMTime`'s contract is `value / timescale == seconds` and `time_base` is ticks-to-seconds
/// (`num / den`), for an arbitrary (not just `num == 1`) rational timebase. See ADR-0001 §
/// Timestamps.
#[must_use]
pub(super) fn cmtime_value_from_ticks(ticks: i64, time_base: Rational) -> (i64, i32) {
    let num = i64::try_from(time_base.num).unwrap_or(i64::MAX);
    let value = ticks.saturating_mul(num);
    let timescale = i32::try_from(time_base.den).unwrap_or(i32::MAX);
    (value, timescale)
}

/// Inverse of [`cmtime_value_from_ticks`] — recovers a tick count in `time_base` units from a
/// `CMTime`'s `(value, timescale)` pair, used by the decompression output callback to convert
/// `VideoToolbox`'s returned `presentationTimeStamp`/`presentationDuration` back into this crate's
/// `VideoFrame::pts`/`duration` convention. Returns `0` on degenerate input (`timescale == 0` or
/// `time_base.num == 0`) rather than dividing by zero.
#[must_use]
pub(super) fn ticks_from_cmtime_value(value: i64, timescale: i32, time_base: Rational) -> i64 {
    if timescale == 0 || time_base.num == 0 {
        return 0;
    }
    // i128 intermediate: avoids overflow on `value * time_base.den` for pathological CMTime
    // values from the decoder, without needing a fallible/panicking path.
    let numerator = i128::from(value) * i128::from(time_base.den);
    let denominator = i128::from(timescale) * i128::from(time_base.num);
    if denominator == 0 {
        return 0;
    }
    let ticks = numerator / denominator;
    i64::try_from(ticks).unwrap_or_else(|_| {
        if ticks.is_negative() {
            i64::MIN
        } else {
            i64::MAX
        }
    })
}

/// Same conversion as [`ticks_from_cmtime_value`], clamped to `u64` for
/// [`VideoFrame::duration`](mediaway_common::VideoFrame::duration) (negative/degenerate results
/// become `0`, matching this crate's "0 == unknown" convention).
#[must_use]
pub(super) fn duration_ticks_from_cmtime_value(
    value: i64,
    timescale: i32,
    time_base: Rational,
) -> u64 {
    u64::try_from(ticks_from_cmtime_value(value, timescale, time_base)).unwrap_or(0)
}

/// Copy stride-padded NV12 Y/UV plane data — **two separate** base-address slices, matching
/// `CVPixelBuffer`'s per-plane layout (unlike VA-API's single contiguous mapped image the
/// `linux::vaapi::nv12` sibling reads from) — into one tightly packed [`Bytes`]. Output layout
/// matches that sibling's convention: `width * height` luma bytes followed by `width * height /
/// 2` interleaved chroma bytes, both tightly packed (stride removed).
///
/// Rows that would read past either plane slice's end are left zeroed (defensive: a
/// driver/OS reporting inconsistent strides should not panic this path).
#[must_use]
pub(super) fn copy_nv12_planes(
    y_plane: &[u8],
    y_stride: usize,
    uv_plane: &[u8],
    uv_stride: usize,
    width: u32,
    height: u32,
) -> Bytes {
    let width = width as usize;
    let height = height as usize;
    let uv_rows = height / 2;

    let mut out = vec![0u8; width * height + width * uv_rows];

    for row in 0..height {
        let src_start = row * y_stride;
        let src_end = src_start + width;
        if src_end > y_plane.len() {
            break;
        }
        let dst_start = row * width;
        out[dst_start..dst_start + width].copy_from_slice(&y_plane[src_start..src_end]);
    }

    let y_plane_bytes = width * height;
    for row in 0..uv_rows {
        let src_start = row * uv_stride;
        let src_end = src_start + width;
        if src_end > uv_plane.len() {
            break;
        }
        let dst_start = y_plane_bytes + row * width;
        out[dst_start..dst_start + width].copy_from_slice(&uv_plane[src_start..src_end]);
    }

    Bytes::from(out)
}

#[cfg(test)]
#[path = "codec_tests.rs"]
mod tests;
