//! Byte <-> interleaved-`f32` PCM conversions shared by [`AudioProcessor`](crate::apm::AudioProcessor)
//! and [`VoiceActivityDetector`](crate::apm::VoiceActivityDetector).
//!
//! `sonora`/`sonora-agc2` take `&[f32]`/`&mut [f32]`; `mediaway_common::AudioFrame`
//! carries raw interleaved bytes. Converting between the two, with
//! `#![forbid(unsafe_code)]` ruling out a pointer-cast reinterpret, is a real
//! per-sample copy — the not-Zero-Copy cost this crate's ADR documents (see
//! `docs/ai/wiki/zero-copy/marks.md`).

#[cfg(feature = "apm")]
use mediaway_common::Bytes;

/// Reinterprets little-endian `f32` bytes as a `Vec<f32>`. Any trailing bytes
/// that don't form a complete `f32` (fewer than 4) are dropped.
#[allow(
    clippy::redundant_pub_crate,
    reason = "pub(crate) here satisfies rustc's unreachable_pub; clippy's \
              redundant_pub_crate and unreachable_pub disagree on the fix"
)]
pub(crate) fn bytes_to_f32(data: &[u8]) -> Vec<f32> {
    data.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Inverse of [`bytes_to_f32`] — packs interleaved `f32` samples back to
/// little-endian bytes. Only [`AudioProcessor`](crate::apm::AudioProcessor)
/// (`apm` feature) produces output frames this way —
/// [`VoiceActivityDetector`](crate::apm::VoiceActivityDetector) only ever
/// consumes bytes via [`bytes_to_f32`].
#[cfg(feature = "apm")]
#[allow(
    clippy::redundant_pub_crate,
    reason = "pub(crate) here satisfies rustc's unreachable_pub; clippy's \
              redundant_pub_crate and unreachable_pub disagree on the fix"
)]
pub(crate) fn f32_to_bytes(samples: &[f32]) -> Bytes {
    let mut buf = Vec::with_capacity(samples.len() * 4);
    for &sample in samples {
        buf.extend_from_slice(&sample.to_le_bytes());
    }
    Bytes::from(buf)
}
