//! Owned-output buffer helpers shared by `video.rs`/`audio.rs`.
//!
//! No borrowed-input helper (`borrow_slice`) exists in this crate — every function in
//! this ABI takes only plain value structs or opaque handles as input; nothing borrows
//! a raw byte buffer from the caller (`adr/0001-capture-c-abi.md` §4). Ownership rules
//! for the owned outputs below: `adr/0001-capture-c-abi.md` §6.
//!
//! `leak_boxed_slice`/`reclaim_boxed_slice` are implemented in `mediaway-common-ffi` and
//! imported here (`docs/adr/0015-common-ffi-unification.md`) — this crate's own exported
//! `_free` function names/signatures are unchanged.

// Re-exported (not just `use`d) so `video.rs`/`audio.rs` can keep referring to
// `crate::device::buffer::{leak_boxed_slice, reclaim_boxed_slice}` unchanged.
// `pub(crate)`, not `pub`, because they are internal-only (never re-exported from
// `lib.rs`) — that is genuinely the intended visibility, not redundancy.
// `clippy::redundant_pub_crate` disagrees with rustc's own `unreachable_pub` lint for this
// exact "pub(crate) item in a private module" shape; rustc's lint wins here.
#![allow(clippy::redundant_pub_crate)]

pub(crate) use crate::common::buffer::{leak_boxed_slice, reclaim_boxed_slice};
