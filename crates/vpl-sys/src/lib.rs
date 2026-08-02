//! Minimal, unprefixed FFI core for a subset of Intel oneVPL (`intel/libvpl`,
//! MIT). No dependency on `mediaway-common` or any Mediaway facade — this
//! crate has no Mediaway-specific types, mirroring `iso-bmff`/`iso-cenc`'s
//! "reusable outside Mediaway" positioning (ADR-0012).
//!
//! # Scope (Stage 1)
//!
//! - Types: [`raw`] — `bindgen`-generated oneVPL struct/union layout from
//!   vendored headers (`vendor/`), exact `#pragma pack` semantics verified by
//!   real Clang parsing rather than hand-transcribed.
//! - Constants: [`consts`] — hand-transcribed `MFX_*` values (status codes,
//!   codec/profile/level/IOPattern/rate-control selectors), each cited
//!   against the vendored header it came from.
//! - Runtime loading: [`dispatcher`] — a **deliberately reduced**
//!   reimplementation of Intel's own oneVPL dispatcher: resolve a
//!   driver-shipped implementation library (`libmfxhw64.dll` on Windows) via
//!   `libloading`, then the ~10 `MFX*` entry points this crate's Stage 1
//!   H.264 CPU-upload encode path needs. **Not** a port of Intel's real
//!   dispatcher (`MFXLoad`/`MFXCreateConfig`/`MFXEnumImplementations`) — no
//!   multi-implementation ranking, no capability filtering, first working
//!   Intel GPU implementation wins. See `dispatcher` module docs and
//!   `mediaway-encoder-quicksync/adr/0001-onevpl-quicksync-encode-surface.md`.
//!
//! No build-time link against any Intel-provided import library — every
//! entry point is resolved at runtime (`GetProcAddress`/`dlsym` via
//! `libloading`), matching how the real oneVPL dispatcher itself works.

#![allow(unsafe_code)]

pub mod consts;
pub mod dispatcher;
pub mod raw;

pub use dispatcher::{Loader, Session, VplError};
