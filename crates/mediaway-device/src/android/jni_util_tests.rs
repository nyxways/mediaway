#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::cast_possible_wrap,
    clippy::significant_drop_tightening,
    reason = "test modules may unwrap / print"
)]

use super::*;

#[test]
fn jni_error_converts_to_backend() {
    let err = JniError::UninitializedJavaVM;
    let mapped: crate::CaptureError = err.into();
    assert_eq!(mapped, crate::CaptureError::Backend);
}
