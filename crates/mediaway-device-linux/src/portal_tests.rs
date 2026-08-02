#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "unit tests may unwrap"
)]

use super::*;
use ashpd::desktop::ResponseError;
use std::pin::Pin;

#[test]
fn response_cancelled_maps_to_access_denied() {
    let err = ashpd::Error::Response(ResponseError::Cancelled);
    assert_eq!(map_ashpd_error(&err), CaptureError::AccessDenied);
}

#[test]
fn no_response_maps_to_backend() {
    assert_eq!(
        map_ashpd_error(&ashpd::Error::NoResponse),
        CaptureError::Backend
    );
}

#[test]
fn io_error_maps_to_backend() {
    let err = ashpd::Error::IO(std::io::Error::other("boom"));
    assert_eq!(map_ashpd_error(&err), CaptureError::Backend);
}

#[test]
fn invalid_app_id_maps_to_unsupported_by_default_arm() {
    assert_eq!(
        map_ashpd_error(&ashpd::Error::InvalidAppID),
        CaptureError::Unsupported
    );
}

#[test]
fn block_on_resolves_an_already_ready_future() {
    assert_eq!(block_on(async { 42 }), 42);
}

#[test]
fn block_on_resolves_after_one_self_wake() {
    // Deterministic Pending -> Ready path: the future wakes its own waker
    // synchronously before returning `Pending` once, so `block_on`'s
    // park/notify cycle runs exactly once with no thread/timing involved.
    struct YieldOnce {
        yielded: bool,
    }
    impl Future for YieldOnce {
        type Output = u32;
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<u32> {
            if self.yielded {
                Poll::Ready(99)
            } else {
                self.yielded = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    assert_eq!(block_on(YieldOnce { yielded: false }), 99);
}
