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
