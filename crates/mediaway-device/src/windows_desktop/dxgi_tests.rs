//! Unit tests for `dxgi.rs` (sibling of the implementation).

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use super::{enumerate_outputs, output_device_name};
use windows::Win32::Graphics::Dxgi::DXGI_OUTPUT_DESC;

/// `output_device_name` is pure — no DXGI I/O — so the nul-termination
/// decoding logic is unit-testable without a live adapter/output, same
/// rationale as `wgc.rs`'s `resized_geometry`.
fn desc_with_name(name: &str) -> DXGI_OUTPUT_DESC {
    let mut buf = [0u16; 32];
    for (dst, src) in buf.iter_mut().zip(name.encode_utf16()) {
        *dst = src;
    }
    DXGI_OUTPUT_DESC {
        DeviceName: buf,
        ..Default::default()
    }
}

#[test]
fn output_device_name_decodes_up_to_first_nul() {
    let desc = desc_with_name("\\\\.\\DISPLAY1");
    assert_eq!(output_device_name(&desc), "\\\\.\\DISPLAY1");
}

#[test]
fn output_device_name_handles_full_buffer_with_no_trailing_nul() {
    // 32 non-nul `u16`s exactly fills `DeviceName` — `position` finds no `0`
    // and must fall back to the whole buffer length, not panic/truncate.
    let desc = DXGI_OUTPUT_DESC {
        DeviceName: [u16::from(b'A'); 32],
        ..Default::default()
    };
    assert_eq!(output_device_name(&desc), "A".repeat(32));
}

#[test]
fn output_device_name_empty_buffer_is_empty_string() {
    let desc = DXGI_OUTPUT_DESC::default();
    assert_eq!(output_device_name(&desc), "");
}

/// Live DXGI output enumeration — this dev machine has a real display (same
/// assumption `capabilities_tests.rs::screen_support_or_skip` makes), so this
/// asserts the real `is_default`/`ordinal` contract rather than just "doesn't
/// crash": ordinal `0` must be `is_default: true`, every other ordinal must
/// be `false`, and `ordinal` must match each entry's position in the
/// returned `Vec` (ADR-0005's enumeration contract).
#[test]
fn enumerate_outputs_ordinal_and_is_default_or_skip() {
    let _guard = crate::windows_desktop::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match enumerate_outputs() {
        Ok(outputs) if outputs.is_empty() => {
            eprintln!("skip: no DXGI outputs enumerated on this machine (headless/CI box?)");
        }
        Ok(outputs) => {
            for (index, info) in outputs.iter().enumerate() {
                assert_eq!(info.ordinal, u32::try_from(index).expect("small index"));
                assert_eq!(info.is_default, index == 0);
                assert!(!info.name.is_empty(), "DXGI DeviceName must not be empty");
            }
        }
        Err(e) => eprintln!("skip: enumerate_outputs failed ({e:?})"),
    }
}
