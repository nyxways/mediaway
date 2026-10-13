//! Bitstream framing helpers (sans-io).

#![forbid(unsafe_code)]

#[cfg(feature = "audio")]
pub mod aac;
#[cfg(feature = "video")]
pub mod av1;
#[cfg(feature = "video")]
pub mod avc;
#[cfg(feature = "video")]
pub mod hevc;

#[cfg(feature = "audio")]
pub use aac::strip_adts;
#[cfg(feature = "video")]
pub use av1::{Av1cOut, to_av1c};
#[cfg(feature = "video")]
pub use avc::{
    AvcDecoderConfig, AvccOut, annex_b_sequence_header, avcc_payload_to_annex_b,
    parse_avc_decoder_config, to_avcc,
};
#[cfg(feature = "video")]
pub use hevc::{HevcDecoderConfig, HvccOut, parse_hevc_decoder_config, to_hvcc};

/// True when `data` is exactly a run of length-prefixed NAL units (AVCC / `hvcC` framing).
///
/// Each NAL comes after a `nal_length_size`-byte big-endian length; the lengths must walk to the
/// end of `data` with no byte left over, no NAL may be empty, and there must be at least one.
///
/// This settles what a start-code probe cannot. A length-prefixed access unit whose first NAL is
/// 256 to 511 bytes long begins `00 00 01 xx` (with 4-byte lengths), which is also an Annex-B
/// 3-byte start code, so [`avc::is_annex_b`] / [`hevc::is_annex_b`] take it for Annex-B and a
/// caller that then leaves it unconverted feeds a decoder garbage. Real Annex-B data does not walk
/// like this by accident: the "length" it starts with is the start code plus the first NAL header
/// byte (at least 256), and the chain would have to land on the last byte exactly.
#[cfg(feature = "video")]
#[must_use]
pub fn is_length_prefixed(data: &[u8], nal_length_size: u8) -> bool {
    let nls = usize::from(nal_length_size);
    if !(1..=4).contains(&nls) {
        return false;
    }
    let (mut pos, mut units) = (0usize, 0usize);
    while pos < data.len() {
        let Some(head) = data.get(pos..pos + nls) else {
            return false;
        };
        let len = head.iter().fold(0usize, |n, b| (n << 8) | usize::from(*b));
        pos += nls;
        if len == 0 || pos + len > data.len() {
            return false;
        }
        pos += len;
        units += 1;
    }
    units > 0
}
