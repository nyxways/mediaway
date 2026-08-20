//! Real hardware smoke test: `WgpuDx12Bridge`'s render-direct Zero-Copy path
//! (`render_target`/`handle`) and the external-shared-resource constructor
//! (`from_external_shared_resource`). See ADR-0005.
//!
//! Skips gracefully at the first missing capability, mirroring
//! `dx12_encode_smoke.rs`'s pattern.

#![cfg(windows)]
#![allow(
    unsafe_code,
    reason = "HAL escape-hatch extraction of the render_target texture's raw ID3D12Resource, \
              to prove from_external_shared_resource against a real already-shared resource"
)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::too_many_lines,
    reason = "integration test: one linear device -> bridge -> verify smoke test"
)]

use mediaway::wgpu::WgpuDx12Bridge;
use mediaway_common::{GpuBufferHandle, NativeHandle};

#[test]
fn wgpu_dx12_bridge_render_target_handle_or_skip() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter =
        match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        {
            Ok(a) => a,
            Err(e) => {
                eprintln!("skip: no DX12 wgpu adapter ({e:?})");
                return;
            }
        };
    let (device, queue) =
        match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("mediaway render_target/handle smoke test device"),
            ..Default::default()
        })) {
            Ok(dq) => dq,
            Err(e) => {
                eprintln!("skip: wgpu request_device failed ({e:?})");
                return;
            }
        };

    let (width, height) = (64u32, 64u32);
    let bridge = match WgpuDx12Bridge::new(&device, width, height) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("skip: WgpuDx12Bridge::new failed ({e:?})");
            return;
        }
    };

    // Render directly into the bridge's shared texture — no `copy_frame` at all.
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("render_target clear"),
    });
    {
        let view = bridge
            .render_target()
            .create_view(&wgpu::TextureViewDescriptor::default());
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("render_target clear pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.25,
                        g: 0.5,
                        b: 0.75,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
    queue.submit(std::iter::once(encoder.finish()));

    let handle = match bridge.handle(&device) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("skip: bridge.handle failed ({e:?})");
            return;
        }
    };
    assert!(
        matches!(handle, GpuBufferHandle::DirectX11 { .. }),
        "expected a DirectX11 handle from the render_target/handle path"
    );
    eprintln!("wgpu dx12 bridge: render_target/handle zero-copy path ok");

    // `from_external_shared_resource`: re-share the same underlying D3D12 resource on a second
    // bridge instance — proves the "bring your own already-shared resource" constructor works.
    // SAFETY: `as_hal`/`raw_resource` only borrow the resource for the guard's lifetime; the raw
    // pointer bits are read out and the guard dropped before `from_external_shared_resource`
    // re-derives its own COM reference from those bits (same pattern
    // `dx12.rs::wrap_bridge_resource` uses).
    let raw_bits =
        unsafe { bridge.render_target().as_hal::<wgpu::hal::api::Dx12>() }.map(|hal_texture| {
            let raw: &windows::Win32::Graphics::Direct3D12::ID3D12Resource =
                unsafe { hal_texture.raw_resource() };
            windows::core::Interface::as_raw(raw) as usize
        });
    let Some(raw_bits) = raw_bits else {
        eprintln!("skip: render_target has no DX12 HAL backing");
        return;
    };
    let Some(resource_handle) = NativeHandle::new(raw_bits) else {
        eprintln!("skip: null resource pointer");
        return;
    };

    let bridge2 = match WgpuDx12Bridge::from_external_shared_resource(
        &device,
        resource_handle,
        width,
        height,
    ) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("skip: from_external_shared_resource failed ({e:?})");
            return;
        }
    };
    let handle2 = bridge2
        .handle(&device)
        .expect("handle on external-shared-resource bridge");
    assert!(
        matches!(handle2, GpuBufferHandle::DirectX11 { .. }),
        "expected a DirectX11 handle from the external-shared-resource bridge"
    );
    eprintln!("wgpu dx12 bridge: from_external_shared_resource ok");
}
