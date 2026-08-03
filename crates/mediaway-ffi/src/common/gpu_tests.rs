//! Pure-logic round-trip tests for the `GpuDeviceHandle`/`GpuBufferHandle` C mirrors
//! (`adr/0003-gpu-handle-c-abi.md` §1). No real device/COM object involved — `native`
//! bits here are arbitrary non-zero integers exercising the conversion logic only.

#![allow(clippy::unwrap_used, reason = "unit tests")]

use super::*;
use mediaway_common::{
    GpuBufferHandle as CommonGpuBufferHandle, GpuDeviceHandle as CommonGpuDeviceHandle,
};

#[test]
fn to_common_none_is_none() {
    let handle = GpuDeviceHandle {
        kind: GpuDeviceKind::None,
        native: 0,
        webgpu_device_id: 0,
    };
    assert_eq!(handle.to_common(), None);
}

#[test]
fn to_common_zero_native_directx11_is_none() {
    // Defensive: a zero-initialized struct with a non-NONE kind (e.g. a language
    // binding that forgot to fill the field) must not be interpreted as a real device.
    let handle = GpuDeviceHandle {
        kind: GpuDeviceKind::DirectX11,
        native: 0,
        webgpu_device_id: 0,
    };
    assert_eq!(handle.to_common(), None);
}

#[test]
fn to_common_directx11_round_trips() {
    let handle = GpuDeviceHandle {
        kind: GpuDeviceKind::DirectX11,
        native: 0x1234,
        webgpu_device_id: 0,
    };
    let common = handle.to_common().unwrap();
    assert_eq!(
        common,
        CommonGpuDeviceHandle::DirectX11(NativeHandle::new(0x1234).unwrap())
    );
}

#[test]
fn to_common_webgpu_ignores_native_uses_device_id() {
    let handle = GpuDeviceHandle {
        kind: GpuDeviceKind::WebGpu,
        native: 0,
        webgpu_device_id: 42,
    };
    assert_eq!(
        handle.to_common(),
        Some(CommonGpuDeviceHandle::WebGpu { device_id: 42 })
    );
}

#[test]
fn from_common_directx11_round_trips() {
    let common = CommonGpuBufferHandle::DirectX11 {
        texture: NativeHandle::new(0xAAAA).unwrap(),
        subresource: 3,
    };
    let handle: GpuBufferHandle = common.into();
    assert_eq!(handle.kind, GpuBufferKind::DirectX11);
    assert_eq!(handle.native_a, 0xAAAA);
    assert_eq!(handle.subresource, 3);
    assert_eq!(handle.native_b, 0);
    assert_eq!(handle.webgpu_texture_id, 0);
}

#[test]
fn from_common_vulkan_round_trips_both_native_fields() {
    let common = CommonGpuBufferHandle::Vulkan {
        image: NativeHandle::new(0x1111).unwrap(),
        memory: NativeHandle::new(0x2222).unwrap(),
    };
    let handle: GpuBufferHandle = common.into();
    assert_eq!(handle.kind, GpuBufferKind::Vulkan);
    assert_eq!(handle.native_a, 0x1111);
    assert_eq!(handle.native_b, 0x2222);
}

#[test]
fn from_common_webgpu_round_trips() {
    let common = CommonGpuBufferHandle::WebGpu { texture_id: 99 };
    let handle: GpuBufferHandle = common.into();
    assert_eq!(handle.kind, GpuBufferKind::WebGpu);
    assert_eq!(handle.webgpu_texture_id, 99);
    assert_eq!(handle.native_a, 0);
}

#[test]
fn from_common_directx_shared_round_trips() {
    let common = CommonGpuBufferHandle::DirectXShared {
        handle: NativeHandle::new(0x55).unwrap(),
    };
    let handle: GpuBufferHandle = common.into();
    assert_eq!(handle.kind, GpuBufferKind::DirectXShared);
    assert_eq!(handle.native_a, 0x55);
}

#[test]
fn buffer_to_common_unknown_is_none() {
    let handle = GpuBufferHandle {
        kind: GpuBufferKind::Unknown,
        native_a: 0xAAAA,
        native_b: 0,
        subresource: 0,
        webgpu_texture_id: 0,
    };
    assert_eq!(handle.to_common(), None);
}

#[test]
fn buffer_to_common_zero_native_directx11_is_none() {
    // Same defensive contract as GpuDeviceHandle::to_common: a zero-initialized struct
    // with a non-Unknown kind must not be interpreted as a real buffer.
    let handle = GpuBufferHandle {
        kind: GpuBufferKind::DirectX11,
        native_a: 0,
        native_b: 0,
        subresource: 3,
        webgpu_texture_id: 0,
    };
    assert_eq!(handle.to_common(), None);
}

#[test]
fn buffer_to_common_directx11_round_trips() {
    let handle = GpuBufferHandle {
        kind: GpuBufferKind::DirectX11,
        native_a: 0xAAAA,
        native_b: 0,
        subresource: 3,
        webgpu_texture_id: 0,
    };
    assert_eq!(
        handle.to_common(),
        Some(CommonGpuBufferHandle::DirectX11 {
            texture: NativeHandle::new(0xAAAA).unwrap(),
            subresource: 3,
        })
    );
}

#[test]
fn buffer_to_common_vulkan_round_trips_both_native_fields() {
    let handle = GpuBufferHandle {
        kind: GpuBufferKind::Vulkan,
        native_a: 0x1111,
        native_b: 0x2222,
        subresource: 0,
        webgpu_texture_id: 0,
    };
    assert_eq!(
        handle.to_common(),
        Some(CommonGpuBufferHandle::Vulkan {
            image: NativeHandle::new(0x1111).unwrap(),
            memory: NativeHandle::new(0x2222).unwrap(),
        })
    );
}

#[test]
fn buffer_to_common_vulkan_zero_memory_is_none() {
    let handle = GpuBufferHandle {
        kind: GpuBufferKind::Vulkan,
        native_a: 0x1111,
        native_b: 0,
        subresource: 0,
        webgpu_texture_id: 0,
    };
    assert_eq!(handle.to_common(), None);
}

#[test]
fn buffer_to_common_webgpu_round_trips() {
    let handle = GpuBufferHandle {
        kind: GpuBufferKind::WebGpu,
        native_a: 0,
        native_b: 0,
        subresource: 0,
        webgpu_texture_id: 99,
    };
    assert_eq!(
        handle.to_common(),
        Some(CommonGpuBufferHandle::WebGpu { texture_id: 99 })
    );
}

#[test]
fn buffer_to_common_directx_shared_round_trips() {
    let handle = GpuBufferHandle {
        kind: GpuBufferKind::DirectXShared,
        native_a: 0x55,
        native_b: 0,
        subresource: 0,
        webgpu_texture_id: 0,
    };
    assert_eq!(
        handle.to_common(),
        Some(CommonGpuBufferHandle::DirectXShared {
            handle: NativeHandle::new(0x55).unwrap(),
        })
    );
}
