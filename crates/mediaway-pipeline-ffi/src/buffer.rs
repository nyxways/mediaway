//! Buffer ownership helpers and the shared `_free` function.
//!
//! Ownership rules: `adr/0001-auto-encode-c-abi.md` §6. `borrow_slice`/
//! `leak_boxed_slice`/`reclaim_boxed_slice` are implemented in `mediaway-common-ffi`
//! and imported here (`docs/adr/0015-common-ffi-unification.md`) — this crate's own
//! exported `mediaway_pipeline_ffi_buffer_free` name/signature is unchanged.

// Re-exported (not just `use`d) so `session.rs` can keep referring to
// `crate::buffer::{borrow_slice, leak_boxed_slice}` unchanged.
// `pub(crate)`, not `pub`, because they are internal-only (never re-exported from
// `lib.rs`) — that is genuinely the intended visibility, not redundancy.
// `clippy::redundant_pub_crate` disagrees with rustc's own `unreachable_pub` lint for this
// exact "pub(crate) item in a private module" shape; rustc's lint wins here.
#![allow(clippy::redundant_pub_crate)]

pub(crate) use mediaway_common_ffi::buffer::{borrow_slice, leak_boxed_slice, reclaim_boxed_slice};

/// Free a buffer returned by [`crate::mediaway_encode_session_finish`].
///
/// Distinctly named from `mediaway-container-ffi`'s `mediaway_buffer_free` — two
/// `-ffi` crates defining the same global C symbol would be a duplicate-symbol
/// link error if both crates' `staticlib` outputs are ever linked into one
/// consumer (`adr/0001-auto-encode-c-abi.md` § Buffer-free naming).
///
/// # Safety
///
/// `data`/`len` must be exactly the pointer/length pair returned by that function (or
/// `(null, 0)`), and must not have already been freed. Thread-confined: do not call
/// concurrently with another call passing the same `data`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_pipeline_ffi_buffer_free(data: *mut u8, len: usize) {
    // SAFETY: caller guarantees `data`/`len` came from `mediaway_encode_session_finish` and
    // are not yet freed (function contract).
    unsafe { reclaim_boxed_slice(data, len) };
}
