//! `ftyp` brand box.

#![forbid(unsafe_code)]

use super::{tag, write_box};
use crate::types::{Codec, Track};

/// Write a standard Progressive/fMP4 `ftyp`.
///
/// The codec-specific compatible brand (`avc1`/`hvc1`/`av01`/`vp09`) reflects
/// `tracks`' first video codec — an audio-only file, or one with no video
/// track using a brand-recognized codec, omits it rather than falsely
/// claiming AVC compatibility (the previous behavior: `avc1` unconditionally,
/// regardless of what `stsd` actually wrote).
pub fn write_ftyp(buf: &mut Vec<u8>, tracks: &[Track]) {
    write_box(buf, tag::FTYP, |b| {
        b.extend_from_slice(b"isom");
        b.extend_from_slice(&512u32.to_be_bytes());
        b.extend_from_slice(b"isom");
        b.extend_from_slice(b"iso2");
        if let Some(brand) = video_codec_brand(tracks) {
            b.extend_from_slice(brand);
        }
        b.extend_from_slice(b"mp41");
    });
}

/// The `stsd` sample-entry fourcc for `tracks`' first video-codec track, if
/// any — matches `sample_entry.rs::write_stsd`'s codec → box-type mapping.
fn video_codec_brand(tracks: &[Track]) -> Option<&'static [u8; 4]> {
    tracks.iter().find_map(|t| match t.codec {
        Codec::H264 => Some(b"avc1"),
        Codec::Hevc => Some(b"hvc1"),
        Codec::Av1 => Some(b"av01"),
        Codec::Vp9 => Some(b"vp09"),
        Codec::Aac | Codec::Opus | Codec::WebVtt | Codec::Tx3g => None,
    })
}

#[cfg(test)]
#[path = "ftyp_tests.rs"]
mod tests;
