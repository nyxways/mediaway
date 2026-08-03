//! Unit tests for `enumeration.rs` (sibling of the implementation).

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use super::enumerate;
use crate::{CaptureError, DeviceKind};

/// `ProcessLoopback` is PID-parameterized at open time, not an OS device
/// list — `enumerate` must return `Unsupported`, never an empty `Vec` (which
/// would falsely read as "no process is producing audio"). Fast, pure logic
/// — no COM/hardware touched (see `enumerate`'s dispatch `match`).
#[test]
fn process_loopback_enumerate_is_unsupported_not_empty() {
    assert!(matches!(
        enumerate(DeviceKind::ProcessLoopback),
        Err(CaptureError::Unsupported)
    ));
}

/// `Window` is out of ADR-0005's enumeration scope entirely (not a
/// persistent device) — same "`Unsupported`, not empty" contract.
#[test]
fn window_enumerate_is_unsupported() {
    assert!(matches!(
        enumerate(DeviceKind::Window),
        Err(CaptureError::Unsupported)
    ));
}

/// Live `WASAPI` capture-endpoint enumeration on this machine. Asserts the
/// real `is_default`/`ordinal` contract (ADR-0005), not just "doesn't
/// crash" — mirrors `capabilities_tests.rs`'s `_or_skip` convention for
/// other real-hardware checks in this crate.
#[test]
fn enumerate_microphone_ordinal_and_is_default_or_skip() {
    let _guard = crate::windows::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match enumerate(DeviceKind::Microphone) {
        Ok(devices) if devices.is_empty() => {
            eprintln!("skip: no microphone endpoints enumerated on this machine");
        }
        Ok(devices) => {
            let default_count = devices.iter().filter(|d| d.is_default).count();
            assert!(
                default_count <= 1,
                "at most one enumerated microphone may be the OS default"
            );
            let names: Vec<&str> = devices.iter().map(|d| d.name.as_str()).collect();
            eprintln!("microphone endpoints: {names:?}");
            for (index, info) in devices.iter().enumerate() {
                assert_eq!(info.ordinal, u32::try_from(index).expect("small index"));
                assert_eq!(info.kind, DeviceKind::Microphone);
                assert!(
                    info.id.as_wasapi_endpoint_id().is_some(),
                    "Microphone DeviceInfo::id must be a WASAPI endpoint id"
                );
            }
        }
        Err(e) => eprintln!("skip: enumerate(Microphone) failed ({e:?})"),
    }
}
