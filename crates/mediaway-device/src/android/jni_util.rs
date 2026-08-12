//! Shared JNI attach helper for [`crate::android::screencast`] — the only Android domain that
//! needs JNI at all (`camera.rs`/`mic.rs` are pure NDK, zero JNI). See
//! [ADR-0003](adr/android/0003-mediaprojection-jni-screen-capture.md) § open questions #2.

#![allow(unsafe_code)]

use jni::JavaVM;
use jni::errors::Error as JniError;

/// Attach the current (worker) thread to `vm_ptr` and run `f` with the resulting [`jni::Env`].
///
/// If the calling thread was not already attached, it is detached again once `f` returns —
/// [`JavaVM::attach_current_thread`]'s own contract. Cheap to call repeatedly from the same
/// thread once it *is* already attached (a thread-local check, no JNI call).
///
/// # Safety
///
/// `vm_ptr` must be a valid, live `JavaVM*` for the whole call. The host-app contract this
/// crate documents (ADR-0003) is that the `JavaVM*` handed into
/// [`crate::android::screencast::AndroidScreenCaptureConfig::java_vm`] outlives the capture
/// session — this function trusts that contract, it cannot verify it.
pub(super) unsafe fn with_attached_env<F, T, E>(vm_ptr: *mut jni::sys::JavaVM, f: F) -> Result<T, E>
where
    F: FnOnce(&mut jni::Env<'_>) -> Result<T, E>,
    E: From<JniError>,
{
    // SAFETY: caller's contract (this fn's own `# Safety`) guarantees `vm_ptr` is valid.
    let vm = unsafe { JavaVM::from_raw(vm_ptr) };
    vm.attach_current_thread(f)
}

/// Every JNI call in `screencast.rs` funnels its `?`-propagated [`JniError`]s through this
/// `From` impl — a Java exception or a JNI-layer failure both surface as an honest
/// [`crate::CaptureError::Backend`] (no NDK-style status code exists here to distinguish finer
/// causes without inspecting the thrown exception's type, which this slice does not do).
impl From<JniError> for crate::CaptureError {
    fn from(_error: JniError) -> Self {
        Self::Backend
    }
}

#[cfg(test)]
#[path = "jni_util_tests.rs"]
mod tests;
