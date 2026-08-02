//! Sans-IO `ClearKey` ISO Common Encryption (sample keystream).
//!
//! Callers own keys and container parsing. This crate only applies AES under
//! ISO/IEC 23001-7 schemes to byte slices. Policy: workspace ADR-0011.
//!
//! Stage 1: [`Scheme::Cenc`] (AES-128-CTR) with optional subsample ranges.

#![forbid(unsafe_code)]
#![allow(clippy::module_name_repetitions)]

mod cenc;
mod error;

pub use cenc::{
    Pattern, Scheme, Subsample, decrypt_cenc, encrypt_cenc, iv_from_8, iv_from_constant,
};
pub use error::Error;
