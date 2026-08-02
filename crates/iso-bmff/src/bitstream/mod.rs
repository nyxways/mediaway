//! Bitstream framing helpers (sans-io).

#![forbid(unsafe_code)]

#[cfg(feature = "audio")]
pub mod aac;
#[cfg(feature = "video")]
pub mod avc;

#[cfg(feature = "audio")]
pub use aac::strip_adts;
#[cfg(feature = "video")]
pub use avc::{
    AvcDecoderConfig, AvccOut, annex_b_sequence_header, avcc_payload_to_annex_b,
    parse_avc_decoder_config, to_avcc,
};
