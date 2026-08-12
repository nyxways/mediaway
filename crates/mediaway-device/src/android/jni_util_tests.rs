use super::*;

#[test]
fn jni_error_converts_to_backend() {
    let err = JniError::UninitializedJavaVM;
    let mapped: crate::CaptureError = err.into();
    assert_eq!(mapped, crate::CaptureError::Backend);
}
