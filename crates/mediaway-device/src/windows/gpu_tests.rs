#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use super::{GpuAdapterSelect, GpuDevice, GpuDeviceOptions, enumerate_gpu_adapters};
use crate::Select;
use crate::desktop::{
    CaptureOutputPreference, CaptureSharing, DesktopCaptureSource, DesktopVideoCapture,
    DesktopVideoCaptureConfig,
};
use crate::windows_desktop::WindowsScreenCapture;
use mediaway_common::Rational;

#[test]
fn enumerate_gpu_adapters_lists_at_least_one_or_skip() {
    let _guard = crate::windows::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let adapters = match enumerate_gpu_adapters() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("skip: enumerate_gpu_adapters failed ({e:?})");
            return;
        }
    };
    if adapters.is_empty() {
        eprintln!("skip: no DXGI adapters on this machine (headless/CI box?)");
        return;
    }
    for adapter in &adapters {
        assert!(!adapter.name.is_empty(), "adapter name must not be empty");
    }
}

#[test]
fn create_default_and_explicit_index_devices_or_skip() {
    let _guard = crate::windows::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let default_device = match GpuDevice::create(GpuDeviceOptions::default()) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("skip: GpuDevice::create(Default) failed ({e:?})");
            return;
        }
    };
    let default_handle = default_device.handle();
    assert!(
        matches!(
            default_handle,
            mediaway_common::GpuDeviceHandle::DirectX11(_)
        ),
        "created device must report a DirectX11 handle"
    );

    let Ok(adapters) = enumerate_gpu_adapters() else {
        eprintln!("skip: enumerate_gpu_adapters failed after a successful create");
        return;
    };
    let Some(hardware_adapter) = adapters.iter().find(|a| a.is_hardware) else {
        eprintln!("skip: no hardware adapter to explicitly select (WARP-only machine?)");
        return;
    };

    // Not every enumerated "hardware" (non-`DXGI_ADAPTER_FLAG_SOFTWARE`) adapter is
    // necessarily D3D11-creatable on every real machine — virtualized/RDP display
    // adapters on some CI/VM environments enumerate as hardware but reject
    // `D3D11CreateDevice` (confirmed on a real `windows-latest` GitHub Actions
    // runner, `Backend`, while the same run's `GpuAdapterSelect::Default` path
    // succeeded fine). Soft-skip rather than hard-fail — this test's job is to prove
    // explicit-index selection is wired correctly, not that every enumerated
    // adapter is universally usable.
    let indexed_device = match GpuDevice::create(GpuDeviceOptions {
        adapter: GpuAdapterSelect::Index(hardware_adapter.index),
        ..GpuDeviceOptions::default()
    }) {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "skip: GpuDevice::create(Index({})) failed ({e:?}) — that adapter may not \
                 be D3D11-creatable on this machine even though it enumerated as hardware",
                hardware_adapter.index
            );
            return;
        }
    };
    assert!(matches!(
        indexed_device.handle(),
        mediaway_common::GpuDeviceHandle::DirectX11(_)
    ));
}

/// The real point of this factory: a device it creates must be genuinely usable by
/// existing Zero-Copy capture code, not just structurally return a non-null handle.
#[test]
fn created_device_drives_real_screen_capture_or_skip() {
    let _guard = crate::windows::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _desktop_guard = crate::windows_desktop::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let device = match GpuDevice::create(GpuDeviceOptions::default()) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("skip: GpuDevice::create failed ({e:?})");
            return;
        }
    };

    let cfg = DesktopVideoCaptureConfig {
        source: DesktopCaptureSource::Screen {
            select: Select::Default,
        },
        time_base: Rational::new(1, 30),
        output: CaptureOutputPreference::ZeroCopyGpu,
        gpu_device: Some(device.handle()),
        sharing: CaptureSharing::Shared,
    };
    let mut cap = match WindowsScreenCapture::open(&cfg) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skip: WindowsScreenCapture::open failed ({e:?}) — DDA unavailable?");
            return;
        }
    };
    let geometry = cap
        .stream_info()
        .geometry()
        .expect("video stream has geometry");
    assert!(geometry.width > 0);
    assert!(geometry.height > 0);
    match cap.poll_frame() {
        Ok(Some(_frame)) => {
            cap.release_frame().expect("release");
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!("skip: poll_frame failed ({e:?})");
            return;
        }
    }
    cap.close().expect("close");
}
