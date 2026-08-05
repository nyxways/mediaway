//! Single C ABI facade over Mediaway container/device/pipeline — merged from the four mediaway-*-ffi crates per ADR-0021
//!
//! Merged from: mediaway-common-ffi, mediaway-container-ffi, mediaway-device-ffi, mediaway-ffi — see
//! ../../docs/adr/0021-workspace-consolidation.md.

#![allow(unsafe_code)] // FFI crate — see docs/conventions/code-style.md § unsafe

pub mod common;
pub mod container;
pub mod device;
#[cfg(feature = "pipeline")]
pub mod pipeline;
