//! Internal-only shared helper for `mediaway-*-ffi` crates.
//!
//! **Not a public C ABI.** This crate is `rlib`-only, ships no `include/` header, and
//! exports zero `#[no_mangle]`/`extern "C"` symbols of its own. It exists purely to
//! remove *Rust-internal* duplication between the `mediaway-*-ffi` crates: the
//! `Rational`/`CodecKind`/`GpuDeviceHandle`/`GpuBufferHandle` `#[repr(C)]` value-type
//! mirrors (+ conversions to/from `mediaway_common`), and the private
//! `borrow_slice`/`leak_boxed_slice`/`reclaim_boxed_slice` buffer-ownership helper
//! implementation.
//!
//! Each consuming crate (`mediaway-container-ffi`, `mediaway-ffi`,
//! `mediaway-device-ffi`) keeps its own status enum, its own exported `_free`
//! function name(s), and its own hand-written header — this crate does not
//! unify those. Design decision: [`docs/adr/0015-common-ffi-unification.md`](
//! ../../../../docs/adr/0015-common-ffi-unification.md).

#![allow(unsafe_code)] // buffer.rs has unsafe pointer-reconstruction logic — see docs/conventions/code-style.md § unsafe

pub mod buffer;
pub mod gpu;
pub mod types;
