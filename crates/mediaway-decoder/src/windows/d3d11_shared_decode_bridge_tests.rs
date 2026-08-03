#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::panic,
    reason = "unit tests may unwrap / panic-on-unexpected-Ok"
)]

use super::*;

#[test]
fn open_rejects_zero_width() {
    // Zero-size check runs before either device handle is dereferenced (ADR-0003 § open
    // step 1), so a bogus non-null handle is safe here — it is never borrowed.
    // `D3d11SharedDecodeBridge` doesn't implement `Debug` (COM wrapper fields), so match
    // instead of `expect_err`/`unwrap_err`.
    let bogus = NativeHandle::new(1).expect("nonzero bits");
    match D3d11SharedDecodeBridge::open(bogus, bogus, 0, 64) {
        Err(e) => assert_eq!(e, DecodeError::InvalidInput),
        Ok(_) => panic!("zero width must be rejected"),
    }
}

#[test]
fn open_rejects_zero_height() {
    let bogus = NativeHandle::new(1).expect("nonzero bits");
    match D3d11SharedDecodeBridge::open(bogus, bogus, 64, 0) {
        Err(e) => assert_eq!(e, DecodeError::InvalidInput),
        Ok(_) => panic!("zero height must be rejected"),
    }
}

/// Hardware-gated smoke test mirroring `mediaway-encoder-windows`'s own
/// `d3d12_shared_bridge_open_or_skip`: open a bridge against a real D3D11 + D3D12 device
/// pair on the *same* explicit adapter, assert `d3d12_resource_handle()` succeeds. Skips
/// gracefully (`eprintln!`, no hard failure) on any missing capability — this workspace's
/// reference machine has no working H.264 decode HW MFT (ADR-0001), so a real
/// decode → bridge → readback round trip is out of scope here (ADR-0003 § Residual risk 7).
#[test]
fn open_same_adapter_or_skip() {
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice,
    };
    use windows::Win32::Graphics::Direct3D12::{D3D12CreateDevice, ID3D12Device};
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1};
    use windows::core::Interface;

    let factory: IDXGIFactory1 = match unsafe { CreateDXGIFactory1() } {
        Ok(f) => f,
        Err(e) => {
            eprintln!("skip: CreateDXGIFactory1 ({e:?})");
            return;
        }
    };
    let adapter: IDXGIAdapter1 = match unsafe { factory.EnumAdapters1(0) } {
        Ok(a) => a,
        Err(e) => {
            eprintln!("skip: EnumAdapters1 ({e:?})");
            return;
        }
    };

    // Same explicit adapter on both sides — D3D_DRIVER_TYPE_HARDWARE / a fresh D3D12
    // adapter pick could otherwise land on different GPUs on a multi-adapter machine.
    let mut d3d11_device: Option<ID3D11Device> = None;
    if unsafe {
        D3D11CreateDevice(
            &adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&raw mut d3d11_device),
            None,
            None,
        )
    }
    .is_err()
    {
        eprintln!("skip: D3D11CreateDevice on explicit adapter failed");
        return;
    }
    let Some(d3d11_device) = d3d11_device else {
        eprintln!("skip: null D3D11 device");
        return;
    };

    let mut d3d12_device: Option<ID3D12Device> = None;
    if unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &raw mut d3d12_device) }
        .is_err()
    {
        eprintln!("skip: D3D12CreateDevice on explicit adapter failed");
        return;
    }
    let Some(d3d12_device) = d3d12_device else {
        eprintln!("skip: null D3D12 device");
        return;
    };

    let Some(handle11) = NativeHandle::new(Interface::as_raw(&d3d11_device) as usize) else {
        eprintln!("skip: null D3D11 device pointer");
        return;
    };
    let Some(handle12) = NativeHandle::new(Interface::as_raw(&d3d12_device) as usize) else {
        eprintln!("skip: null D3D12 device pointer");
        return;
    };

    match D3d11SharedDecodeBridge::open(handle11, handle12, 64, 64) {
        Ok(bridge) => {
            assert!(bridge.d3d12_resource_handle().is_ok());
            eprintln!("d3d11 shared decode bridge ok");
        }
        Err(e) => eprintln!("skip: D3d11SharedDecodeBridge::open ({e:?})"),
    }
}
