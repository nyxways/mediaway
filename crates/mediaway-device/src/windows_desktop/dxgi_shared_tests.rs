//! Unit + hardware-gated tests for `dxgi_shared.rs` (sibling of the implementation).

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use super::{attach, detach, poll_shared_frame, release_shared_frame};
use crate::windows_desktop::dxgi::enumerate_outputs;
use crate::{CaptureError, DeviceId};
use mediaway_common::{GpuBufferHandle, VideoFrameStorage};
use std::time::Duration;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
};
use windows::core::Interface;

/// Real D3D11 device for hardware-gated tests — same creation call
/// `lib_tests.rs::open_screen_zero_copy_poll_release_or_skip` already uses.
fn create_test_device() -> Option<ID3D11Device> {
    let mut device: Option<ID3D11Device> = None;
    // SAFETY: standard D3D11CreateDevice call with no adapter/context/feature-level
    // out-params retained beyond what's captured here.
    let hr = unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&raw mut device),
            None,
            None,
        )
    };
    if hr.is_err() {
        return None;
    }
    device
}

/// Attaching two consumers to the same output must both succeed (the whole
/// point of ADR-0006), and both must be able to independently poll/release a
/// frame without corrupting the other's state.
#[test]
fn attach_twice_to_same_output_both_succeed_or_skip() {
    let _guard = crate::windows_desktop::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let Some(device) = create_test_device() else {
        eprintln!("skip: no D3D11 hardware device available on this machine");
        return;
    };
    let Ok(outputs) = enumerate_outputs() else {
        eprintln!("skip: DXGI output enumeration failed");
        return;
    };
    if outputs.is_empty() {
        eprintln!("skip: no DXGI outputs on this machine (headless/CI box?)");
        return;
    }

    let device_raw = Interface::as_raw(&device) as usize;
    let key = DeviceId::from_dxgi_output_device_name(outputs[0].name.clone());

    let Ok((shared_a, consumer_a, _info_a)) = attach(key.clone(), device_raw, 0) else {
        eprintln!("skip: first attach failed — no live/duplicable output here");
        return;
    };

    let second = attach(key, device_raw, 0);
    let (shared_b, consumer_b, _info_b) =
        second.expect("second attach on the same output must succeed, not AccessDenied");

    let mut pts_a = 0i64;
    let mut pts_b = 0i64;
    let deadline = std::time::Instant::now() + Duration::from_millis(500);
    let (mut frame_a, mut frame_b) = (None, None);
    while std::time::Instant::now() < deadline && (frame_a.is_none() || frame_b.is_none()) {
        if frame_a.is_none() {
            frame_a = poll_shared_frame(&shared_a, consumer_a, &mut pts_a).unwrap_or(None);
        }
        if frame_b.is_none() {
            frame_b = poll_shared_frame(&shared_b, consumer_b, &mut pts_b).unwrap_or(None);
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    if let Some(frame) = frame_a {
        assert!(matches!(
            frame.storage,
            VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 { .. })
        ));
        release_shared_frame(&shared_a, consumer_a).expect("release consumer A");
    } else {
        eprintln!("skip: consumer A got no frame within 500ms (static desktop?)");
    }
    if let Some(frame) = frame_b {
        assert!(matches!(
            frame.storage,
            VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 { .. })
        ));
        release_shared_frame(&shared_b, consumer_b).expect("release consumer B");
    } else {
        eprintln!("skip: consumer B got no frame within 500ms (static desktop?)");
    }

    detach(&shared_a, consumer_a);
    detach(&shared_b, consumer_b);
    drop(shared_a);
    drop(shared_b);
}

/// Attaching with a different device instance than the one that created the
/// shared session must be rejected, not silently accepted.
#[test]
fn attach_with_mismatched_device_is_invalid_input_or_skip() {
    let _guard = crate::windows_desktop::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let (Some(device_a), Some(device_b)) = (create_test_device(), create_test_device()) else {
        eprintln!("skip: no D3D11 hardware device available on this machine");
        return;
    };
    let Ok(outputs) = enumerate_outputs() else {
        eprintln!("skip: DXGI output enumeration failed");
        return;
    };
    if outputs.is_empty() {
        eprintln!("skip: no DXGI outputs on this machine");
        return;
    }

    let raw_a = Interface::as_raw(&device_a) as usize;
    let raw_b = Interface::as_raw(&device_b) as usize;
    let key = DeviceId::from_dxgi_output_device_name(outputs[0].name.clone());

    let Ok((shared_a, consumer_a, _)) = attach(key.clone(), raw_a, 0) else {
        eprintln!("skip: first attach failed — no live/duplicable output here");
        return;
    };

    let result = attach(key, raw_b, 0);
    assert_eq!(result.err(), Some(CaptureError::InvalidInput));

    detach(&shared_a, consumer_a);
}
