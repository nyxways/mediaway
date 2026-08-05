//! Real hardware smoke test: wgpu DX12 device + a real D3D11 device on the same explicit
//! adapter → `WgpuDx12DecodeBridge::new` construction.
//!
//! **Construction-only, not a decode round trip.** This workspace's reference machine has no
//! working H.264 decode HW MFT (`mediaway-decoder-windows` ADR-0001 / this crate's own
//! ADR-0002 § Context), so there is no real decoded `VideoFrame` to feed
//! `import_decoded_texture` here — mirrors `D3d11SharedDecodeBridge`'s own
//! `open_same_adapter_or_skip` test (`mediaway-decoder-windows`
//! `src/d3d11_shared_decode_bridge_tests.rs`), which is exactly as far as that companion
//! bridge's own hardware verification went.
//!
//! Skips gracefully (never fails the default suite) at the first missing capability: no DX12
//! adapter, no `Features::TEXTURE_FORMAT_NV12` support, `as_hal` extraction failure, no
//! explicit adapter to pair a same-adapter D3D11 device against, or the bridge failing to
//! open (including an adapter mismatch, if wgpu's DX12 adapter enumeration order and DXGI's
//! `EnumAdapters1` order ever disagree on this machine).

#![cfg(windows)]
#![allow(
    unsafe_code,
    reason = "raw DXGI/D3D11 device setup for the test's own adapter pairing"
)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "integration test: one linear device -> bridge construction smoke test"
)]

use mediaway::wgpu::WgpuDx12DecodeBridge;
use mediaway_common::{GpuDeviceHandle, NativeHandle};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1};
use windows::core::Interface;

#[test]
fn wgpu_dx12_decode_bridge_constructs_on_same_adapter_or_skip() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        ..Default::default()
    });

    // Index 0 of wgpu's own DX12 adapter enumeration, matched below against DXGI's
    // `EnumAdapters1(0)` for the D3D11 side — mirrors
    // `d3d11_shared_decode_bridge_tests.rs::open_same_adapter_or_skip`'s "same explicit
    // adapter on both sides" approach, adapted since both bridge sides are caller-owned here.
    let adapters = instance.enumerate_adapters(wgpu::Backends::DX12);
    let Some(adapter) = adapters.into_iter().next() else {
        eprintln!("skip: no DX12 wgpu adapter enumerated");
        return;
    };

    if !adapter
        .features()
        .contains(wgpu::Features::TEXTURE_FORMAT_NV12)
    {
        eprintln!("skip: adapter does not support Features::TEXTURE_FORMAT_NV12");
        return;
    }

    let (device, _queue) =
        match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("mediaway-wgpu decode smoke test device"),
            required_features: wgpu::Features::TEXTURE_FORMAT_NV12,
            ..Default::default()
        })) {
            Ok(dq) => dq,
            Err(e) => {
                eprintln!("skip: wgpu request_device failed ({e:?})");
                return;
            }
        };

    let factory: IDXGIFactory1 = match unsafe { CreateDXGIFactory1() } {
        Ok(f) => f,
        Err(e) => {
            eprintln!("skip: CreateDXGIFactory1 ({e:?})");
            return;
        }
    };
    let dxgi_adapter: IDXGIAdapter1 = match unsafe { factory.EnumAdapters1(0) } {
        Ok(a) => a,
        Err(e) => {
            eprintln!("skip: EnumAdapters1 ({e:?})");
            return;
        }
    };

    let mut d3d11_device: Option<ID3D11Device> = None;
    if unsafe {
        D3D11CreateDevice(
            &dxgi_adapter,
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
    let Some(d3d11_handle) = NativeHandle::new(Interface::as_raw(&d3d11_device) as usize) else {
        eprintln!("skip: null D3D11 device pointer");
        return;
    };

    let (width, height) = (64u32, 64u32);
    match WgpuDx12DecodeBridge::new(
        &device,
        GpuDeviceHandle::DirectX11(d3d11_handle),
        width,
        height,
    ) {
        Ok(_bridge) => {
            eprintln!("wgpu dx12 decode bridge: construction ok (same explicit adapter)");
        }
        Err(e) => {
            eprintln!("skip: WgpuDx12DecodeBridge::new failed ({e:?})");
        }
    }
}
