//! Browser device capture bindings — wasm32 + host stub.

#![forbid(unsafe_code)]

mod config;

pub use config::{DisplayCapturePreferences, UserMediaPreferences};

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub use wasm::*;

#[cfg(not(target_arch = "wasm32"))]
mod host;
#[cfg(not(target_arch = "wasm32"))]
pub use host::*;
