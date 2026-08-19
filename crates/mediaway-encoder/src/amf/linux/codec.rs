//! Pure byte-math helpers for the AMD AMF session: [`VideoEncoderConfig`] → `EncoderConfig`
//! field conversions. No `shiguredo_amf` types here so these stay testable independent of
//! any real AMF library / AMD driver being present — see
//! [ADR-0002](../../adr/amf/0002-amf-linux-shiguredo-amf-h264-cpu-upload.md).

use crate::EncodeError;
use mediaway_common::{CodecKind, Rational};

/// Whether this backend's video encode path accepts `codec` (H.264 / HEVC / AV1 — see
/// [ADR-0003](../../adr/amf/0003-amf-linux-hevc-av1-codec-dispatch.md)). VP9 stays
/// unsupported: `shiguredo_amf`'s own `CodecConfig` has no VP9 variant to dispatch to (ADR-0003
/// § Context), not a Mediaway-side restriction.
#[must_use]
pub(super) const fn is_supported_video_codec(codec: CodecKind) -> bool {
    matches!(codec, CodecKind::H264 | CodecKind::Hevc | CodecKind::Av1)
}

/// `EncoderConfig::framerate_num`/`framerate_den` from a seconds-per-tick [`Rational`]
/// timebase — framerate is the reciprocal of a seconds-per-tick timebase, so
/// `framerate_num = time_base.den`, `framerate_den = time_base.num` (ADR-0002 § Research).
/// [`Rational::den`] is already `u32`, used directly; [`Rational::num`] is `u64` and
/// saturates to `u32::MAX` on overflow (timebases in practice fit comfortably in `u32` —
/// this only guards the type conversion).
#[must_use]
pub(super) fn framerate_from_time_base(time_base: Rational) -> (u32, u32) {
    let den = u32::try_from(time_base.num).unwrap_or(u32::MAX);
    (time_base.den, den.max(1))
}

/// Bits-per-second → `EncoderConfig::target_kbps`/`ReconfigureParams::target_kbps` (`bps /
/// 1000`), `0` bps mapping to `0` kbps (caller decides whether that means "unset").
#[must_use]
pub(super) const fn bps_to_kbps(bitrate_bps: u32) -> u32 {
    bitrate_bps / 1000
}

/// VBV buffer size in bytes → an approximate `max_kbps` ceiling (`bytes * 8 / 1000`,
/// i.e. treat the byte count as a peak burst size measured in kilobits) — the same
/// bytes-to-bits convention this crate's D3D12/Vulkan backends already use for
/// `RateControlConfig::vbv_buffer_size_bytes` (see `windows/d3d12_video_encode/setup.rs`'s
/// `cbr_from_config`), not a driver-specific AMF unit. `shiguredo_amf` gives no documented
/// alternative conversion for this field — ADR-0002 flags this as a "TBD in the
/// implementation PR" candidate; this is that implementation's chosen, honest
/// approximation, not a confirmed AMF semantic.
#[must_use]
pub(super) fn vbv_bytes_to_max_kbps(vbv_buffer_size_bytes: u32) -> u32 {
    let bits = u64::from(vbv_buffer_size_bytes) * 8;
    u32::try_from(bits / 1000).unwrap_or(u32::MAX)
}

/// Tightly-packed NV12 byte size for `width x height` (`width * height * 3 / 2`).
pub(super) fn nv12_size(width: u32, height: u32) -> Result<usize, EncodeError> {
    let w = usize::try_from(width).map_err(|_| EncodeError::InvalidInput)?;
    let h = usize::try_from(height).map_err(|_| EncodeError::InvalidInput)?;
    w.checked_mul(h)
        .and_then(|y| y.checked_mul(3))
        .and_then(|v| v.checked_div(2))
        .ok_or(EncodeError::InvalidInput)
}

#[cfg(test)]
#[path = "codec_tests.rs"]
mod tests;
