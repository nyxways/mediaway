//! Buffer ownership helpers shared by every `mediaway-*-ffi` crate's own exported
//! `_free` function(s).
//!
//! `pub` (not `pub(crate)`) because these now cross a crate boundary — each
//! `mediaway-*-ffi` crate's own `#[no_mangle] extern "C"` free function calls into
//! these, keeping its own name/signature unchanged
//! (`docs/adr/0015-common-ffi-unification.md`).
//!
//! Moved here verbatim from `mediaway-container-ffi`/`mediaway-pipeline-ffi` (byte-for-byte
//! identical private helpers in both). `mediaway-device-ffi` only uses
//! [`leak_boxed_slice`]/[`reclaim_boxed_slice`] — it has no borrowed-input parameters, so it
//! never needed [`borrow_slice`].

/// Build a borrowed slice from a raw pointer + length pair.
///
/// Null with `len == 0` is a valid empty slice; null with a non-zero length is rejected
/// (`None`) so the caller can return an `InvalidArgument`-equivalent status.
///
/// # Safety
///
/// If `ptr` is non-null, it must be valid for reads of `len` bytes for the lifetime `'a`.
pub const unsafe fn borrow_slice<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if ptr.is_null() {
        return if len == 0 { Some(&[]) } else { None };
    }
    // SAFETY: caller guarantees `ptr` is valid for reads of `len` bytes (function contract).
    Some(unsafe { std::slice::from_raw_parts(ptr, len) })
}

/// Leak an owned buffer into a raw pointer + length pair for a `_free` function to reclaim.
///
/// An empty buffer is represented as `(null, 0)`.
pub fn leak_boxed_slice(data: Vec<u8>) -> (*mut u8, usize) {
    if data.is_empty() {
        return (std::ptr::null_mut(), 0);
    }
    let boxed = data.into_boxed_slice();
    let len = boxed.len();
    (Box::into_raw(boxed).cast::<u8>(), len)
}

/// Reclaim and drop a buffer previously leaked by [`leak_boxed_slice`].
///
/// # Safety
///
/// `ptr`/`len` must be exactly as returned by [`leak_boxed_slice`] (or `(null, 0)`), and
/// must not have already been reclaimed.
pub unsafe fn reclaim_boxed_slice(ptr: *mut u8, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    let slice_ptr = std::ptr::slice_from_raw_parts_mut(ptr, len);
    // SAFETY: caller guarantees `ptr`/`len` came from `leak_boxed_slice` and are not yet
    // freed (function contract).
    drop(unsafe { Box::from_raw(slice_ptr) });
}
