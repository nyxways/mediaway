//! Real hardware test: a full `WgpuDx12DecodeBridge` pixel round trip, byte-exact.
//!
//! `dx12_decode_smoke.rs` only proves `WgpuDx12DecodeBridge::new` construction — it never
//! calls `import_decoded_texture`, since this workspace's reference machine has no working
//! H.264 decode HW MFT to produce a real decoded `VideoFrame` from. This test sidesteps that
//! gap: `WgpuDx12DecodeBridge::import_decoded_texture` only cares that it receives a valid,
//! real D3D11 NV12 texture + subresource — it has no way to know (and does not care) whether
//! that texture came from a hardware decoder or from an ordinary CPU write. So this test
//! creates its own **separate, ordinary** D3D11 NV12 texture on the SAME `ID3D11Device` the
//! bridge is opened with, writes a known, non-trivial pixel pattern into it directly from the
//! CPU (staging texture `Map`/memcpy/`Unmap` + `CopyResource`, the same proven pattern
//! `mediaway-encoder`'s `nvenc::dx11::device::upload_cpu_nv12` already uses on this hardware),
//! and feeds it through the bridge exactly like a real decoder output would be.
//!
//! **Readback does not use `wgpu::CommandEncoder::copy_texture_to_buffer` with
//! `TextureAspect::Plane0`/`Plane1`, despite that being the natural wgpu-side approach** — on
//! real hardware this session, that call panics inside `wgpu-hal` 26.0.6's DX12 backend
//! (`wgpu_hal::dx12::Texture::calc_subresource_for_copy` has no match arm for
//! `FormatAspects::PLANE_0`/`PLANE_1`, only `COLOR`/`DEPTH`/`STENCIL` — confirmed by direct
//! source inspection, not a guess; see this file's own doc below and
//! `adr/wgpu/0002-decode-to-wgpu-texture-bridge.md`'s addendum for the exact panic). That is a
//! genuine upstream `wgpu-hal` gap in the pinned DX12 backend, not a Mediaway bug, and applies
//! to `TextureAspect::All` too (multi-planar formats resolve to a multi-bit `FormatAspects`
//! value that the same `unreachable!()` catches). Instead, this test reaches past `wgpu` again
//! (the same "HAL escape hatch" idiom `dx12_decode.rs`/`dx12.rs` already use) to recover the
//! raw `ID3D12Resource` behind the bridge's returned `wgpu::Texture`, opens it as a fresh
//! native D3D11 texture (`ID3D12Device::CreateSharedHandle` → `ID3D11Device1::OpenSharedResource1`,
//! the reverse direction of `D3d11SharedDecodeBridge::open`'s own D3D11→D3D12 share), and reads
//! it back via an ordinary D3D11 staging `Map`. This still exercises — and byte-exact asserts
//! against — the real result of the full pipeline under test: D3D11 CPU write →
//! `D3d11SharedDecodeBridge::copy_from_decoded` (D3D11→D3D11 `CopySubresourceRegion` + query
//! poll) → `WgpuDx12DecodeBridge::import_decoded_texture`'s D3D11→D3D12 shared-handle wrap. It
//! only changes how the test *itself* reads the final bytes back off the GPU, since `wgpu`'s
//! own client-side read path is what is blocked here, not anything in Mediaway.
//!
//! Skips gracefully (never fails the default suite) only at genuinely missing
//! capability/environment steps: no DX12 adapter, no `Features::TEXTURE_FORMAT_NV12`,
//! `D3D11CreateDevice` failure, or `WgpuDx12DecodeBridge::new` construction failure — mirrors
//! `dx12_decode_smoke.rs`'s own skip list. Past that point (a real bridge + a real same-device
//! NV12 texture with known content), `import_decoded_texture` and the pixel-value comparison
//! are hard assertions, not soft skips.

#![cfg(windows)]
#![allow(
    unsafe_code,
    reason = "raw DXGI/D3D11/D3D12 device + texture setup for the test's own stand-in decode texture and its D3D12->D3D11 readback"
)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    reason = "integration test: one linear device -> stand-in decode texture -> bridge -> byte-exact readback"
)]

use mediaway::wgpu::WgpuDx12DecodeBridge;
use mediaway_common::{
    GpuBufferHandle, GpuDeviceHandle, NativeHandle, PixelFormat, VideoFrame, VideoFrameStorage,
};
use windows::Win32::Foundation::{CloseHandle, GENERIC_ALL, HMODULE};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_READ, D3D11_CPU_ACCESS_WRITE,
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_MAP_READ, D3D11_MAP_WRITE, D3D11_MAPPED_SUBRESOURCE,
    D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
    D3D11CreateDevice, ID3D11Device, ID3D11Device1, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Direct3D12::ID3D12Device;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1};
use windows::core::{Interface, PCWSTR};

/// Deterministic, non-trivial NV12 byte pattern: `width*height` luma bytes (varies with both
/// coordinates, catching a transposed or partially-copied plane), followed by
/// `width*height/2` interleaved chroma bytes (`U`/`V` use different formulas, catching a
/// channel swap). Tightly packed (no row-pitch padding) — the same layout
/// `D3d11SharedDecodeBridge`'s planar `CopySubresourceRegion` assumes.
fn build_nv12_pattern(width: u32, height: u32) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((width * height + width * height / 2) as usize);
    for y in 0..height {
        for x in 0..width {
            pixels.push(
                (x.wrapping_mul(3)
                    .wrapping_add(y.wrapping_mul(5))
                    .wrapping_add(7)) as u8,
            );
        }
    }
    for cy in 0..height / 2 {
        for cx in 0..width / 2 {
            pixels.push(
                (cx.wrapping_mul(11)
                    .wrapping_add(cy.wrapping_mul(13))
                    .wrapping_add(1)) as u8,
            );
            pixels.push(
                (cx.wrapping_mul(17)
                    .wrapping_add(cy.wrapping_mul(19))
                    .wrapping_add(2)) as u8,
            );
        }
    }
    pixels
}

/// Allocate an NV12 `ID3D11Texture2D` — shared shape for the stand-in decode-output texture,
/// its CPU-write staging texture, and its CPU-read staging texture.
fn create_nv12_texture(
    device: &ID3D11Device,
    width: u32,
    height: u32,
    usage: D3D11_USAGE,
    cpu_access_flags: u32,
    bind_flags: u32,
) -> ID3D11Texture2D {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_NV12,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: usage,
        BindFlags: bind_flags,
        MiscFlags: 0,
        CPUAccessFlags: cpu_access_flags,
    };
    let mut texture: Option<ID3D11Texture2D> = None;
    // SAFETY: CreateTexture2D with `None` initial data on a live D3D11 device.
    unsafe {
        device
            .CreateTexture2D(&raw const desc, None, Some(&raw mut texture))
            .expect("CreateTexture2D for an NV12 test texture");
    }
    texture.expect("CreateTexture2D returned a null texture")
}

/// Write `pixels` (tightly packed NV12, [`build_nv12_pattern`]'s layout) into `dest` via a
/// `D3D11_USAGE_STAGING` intermediate — the same `Map(WRITE)`/memcpy-per-row/`Unmap` +
/// `CopyResource` pattern `mediaway-encoder`'s `nvenc::dx11::device::upload_cpu_nv12` already
/// hardware-verifies on this reference machine, reused here instead of an unverified direct
/// `UpdateSubresource` call.
fn write_nv12_via_staging(
    context: &ID3D11DeviceContext,
    staging: &ID3D11Texture2D,
    dest: &ID3D11Texture2D,
    pixels: &[u8],
    width: u32,
    height: u32,
) {
    let w = width as usize;
    let h = height as usize;
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: `staging` was created with `D3D11_CPU_ACCESS_WRITE`; `mapped` is a valid stack
    // out-param.
    unsafe {
        context
            .Map(staging, 0, D3D11_MAP_WRITE, 0, Some(&raw mut mapped))
            .expect("Map(WRITE) on the write-side staging texture");
    }
    let row_pitch = mapped.RowPitch as usize;
    // SAFETY: `mapped.pData` is valid for `row_pitch * (h + h/2)` bytes for the duration of
    // this map; every write below stays within one row (`w` bytes) at row index `< h` (luma)
    // or `< h/2` (chroma), and the driver-reported `row_pitch >= width` for a successfully
    // mapped NV12 texture — identical bound `upload_cpu_nv12` already relies on.
    unsafe {
        let base = mapped.pData.cast::<u8>();
        for row in 0..h {
            let src = &pixels[row * w..row * w + w];
            let row_out = std::slice::from_raw_parts_mut(base.add(row * row_pitch), w);
            row_out.copy_from_slice(src);
        }
        let uv_base = base.add(row_pitch * h);
        for row in 0..h / 2 {
            let src_off = w * h + row * w;
            let src = &pixels[src_off..src_off + w];
            let row_out = std::slice::from_raw_parts_mut(uv_base.add(row * row_pitch), w);
            row_out.copy_from_slice(src);
        }
    }
    // SAFETY: ends the map established above.
    unsafe { context.Unmap(staging, 0) };
    // SAFETY: `staging` and `dest` share dimensions/format (`DXGI_FORMAT_NV12`), the contract
    // `CopyResource` requires.
    unsafe { context.CopyResource(dest, staging) };
}

/// Read `src` back into a tightly packed NV12 `Vec<u8>` via a `D3D11_USAGE_STAGING`
/// intermediate — the read-direction mirror of [`write_nv12_via_staging`].
fn read_nv12_via_staging(
    context: &ID3D11DeviceContext,
    staging: &ID3D11Texture2D,
    src: &ID3D11Texture2D,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    // SAFETY: `staging` and `src` share dimensions/format, the contract `CopyResource`
    // requires.
    unsafe { context.CopyResource(staging, src) };

    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: `staging` was created with `D3D11_CPU_ACCESS_READ`; `mapped` is a valid stack
    // out-param.
    unsafe {
        context
            .Map(staging, 0, D3D11_MAP_READ, 0, Some(&raw mut mapped))
            .expect("Map(READ) on the read-side staging texture");
    }
    let row_pitch = mapped.RowPitch as usize;
    let mut out = Vec::with_capacity(w * h + w * (h / 2));
    // SAFETY: `mapped.pData` is valid for `row_pitch * (h + h/2)` bytes for the duration of
    // this map; every read below stays within one row (`w` bytes) at row index `< h` (luma) or
    // `< h/2` (chroma), mirroring [`write_nv12_via_staging`]'s own bound.
    unsafe {
        let base = mapped.pData.cast::<u8>();
        for row in 0..h {
            let row_slice = std::slice::from_raw_parts(base.add(row * row_pitch), w);
            out.extend_from_slice(row_slice);
        }
        let uv_base = base.add(row_pitch * h);
        for row in 0..h / 2 {
            let row_slice = std::slice::from_raw_parts(uv_base.add(row * row_pitch), w);
            out.extend_from_slice(row_slice);
        }
    }
    // SAFETY: ends the map established above.
    unsafe { context.Unmap(staging, 0) };
    out
}

#[test]
fn wgpu_dx12_decode_bridge_pixel_roundtrip_or_skip() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });

    // `enumerate_adapters` became async in wgpu 30 (previously synchronous) — blocked on here,
    // same `pollster` pairing this test already uses for `request_adapter`/`request_device`.
    let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::DX12));
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
            label: Some("mediaway-wgpu decode pixel roundtrip test device"),
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
    let mut d3d11_context: Option<ID3D11DeviceContext> = None;
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
            Some(&raw mut d3d11_context),
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
    let Some(d3d11_context) = d3d11_context else {
        eprintln!("skip: null D3D11 immediate context");
        return;
    };
    let Some(d3d11_device_handle) = NativeHandle::new(Interface::as_raw(&d3d11_device) as usize)
    else {
        eprintln!("skip: null D3D11 device pointer");
        return;
    };

    let (width, height) = (64u32, 64u32);

    // Bridge construction — same environment/capability skip class as `dx12_decode_smoke.rs`.
    let bridge = match WgpuDx12DecodeBridge::new(
        &device,
        GpuDeviceHandle::DirectX11(d3d11_device_handle),
        width,
        height,
    ) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("skip: WgpuDx12DecodeBridge::new failed ({e:?})");
            return;
        }
    };

    // Stand-in "decoder output": an ordinary D3D11 NV12 texture on the SAME d3d11_device the
    // bridge was opened with (copy_from_decoded's own cross-device GetDevice() guard requires
    // this), written from the CPU with a known, non-trivial pattern.
    let source_texture = create_nv12_texture(
        &d3d11_device,
        width,
        height,
        D3D11_USAGE_DEFAULT,
        0,
        D3D11_BIND_SHADER_RESOURCE.0 as u32,
    );
    let write_staging = create_nv12_texture(
        &d3d11_device,
        width,
        height,
        D3D11_USAGE_STAGING,
        D3D11_CPU_ACCESS_WRITE.0 as u32,
        0,
    );
    let pixels = build_nv12_pattern(width, height);
    write_nv12_via_staging(
        &d3d11_context,
        &write_staging,
        &source_texture,
        &pixels,
        width,
        height,
    );

    let Some(source_handle) = NativeHandle::new(Interface::as_raw(&source_texture) as usize) else {
        eprintln!("skip: null source texture pointer");
        return;
    };

    let frame = VideoFrame {
        pts: 0,
        duration: 0,
        width,
        height,
        format: PixelFormat::Nv12,
        storage: VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 {
            texture: source_handle,
            subresource: 0,
        }),
    };

    // Past this point we have a real, constructed bridge and a real same-device D3D11 NV12
    // texture with known content — the real path under test, not an environment-missing gap,
    // so failures here are hard test failures, not skips.
    let imported = bridge
        .import_decoded_texture(&frame)
        .expect("import_decoded_texture must succeed: real bridge, real same-device NV12 texture");

    // Recover the raw ID3D12Resource behind `imported` — same `Device::as_hal` HAL-escape-hatch
    // idiom `dx12_decode.rs::new` already uses, applied to a *texture* instead of a device (see
    // this file's own top doc comment for why `wgpu`'s own copy_texture_to_buffer cannot be
    // used for this readback).
    // SAFETY: `imported` is alive for this whole scope; the guard `as_hal` returns is dropped
    // immediately after reading the raw pointer bits below, never used to mutate the texture.
    let native_texture = unsafe { imported.as_hal::<wgpu::hal::api::Dx12>() }
        .expect("imported texture must be backed by wgpu's DX12 HAL");
    // SAFETY: `raw_resource()` borrows a live COM object owned by `native_texture`'s guard,
    // read only for its pointer bits via `Interface::as_raw` — `wgpu-hal` 30.x pins the same
    // `windows` 0.62 line this file already imports (see `dx12.rs`'s now-resolved straddle
    // note), so no separately-versioned `windows-hal-interop` crate is needed here anymore.
    let resource_bits = unsafe { Interface::as_raw(native_texture.raw_resource()) as usize };
    drop(native_texture);
    let resource_ptr = resource_bits as *mut core::ffi::c_void;
    // SAFETY: `resource_bits` is the exact pointer bits of the live ID3D12Resource wgpu's DX12
    // HAL texture owns; `from_raw_borrowed` + `.clone()` AddRefs an independent COM reference
    // this test owns for the readback — the same cross-`windows`-version pointer-bit bridge
    // `D3d11SharedDecodeBridge`'s own `wrap_bridge_resource`-style code already relies on
    // (`NativeHandle` bits are ABI-stable across `windows`-crate versions for the same COM
    // interface, only the Rust *type* differs).
    let borrowed12 = unsafe {
        windows::Win32::Graphics::Direct3D12::ID3D12Resource::from_raw_borrowed(&resource_ptr)
    }
    .expect("null D3D12 resource pointer");
    let resource12: windows::Win32::Graphics::Direct3D12::ID3D12Resource = borrowed12.clone(); // clone: COM AddRef across windows-crate version boundary, see SAFETY comment above

    let mut device12: Option<ID3D12Device> = None;
    // SAFETY: GetDevice is a simple accessor on a live D3D12 resource.
    unsafe {
        resource12
            .GetDevice(&raw mut device12)
            .expect("ID3D12Resource::GetDevice");
    }
    let device12 = device12.expect("GetDevice returned a null ID3D12Device");

    // Open the SAME underlying GPU allocation as a fresh native D3D11 texture — the reverse
    // direction of `D3d11SharedDecodeBridge::open`'s own D3D11->D3D12 share — purely so this
    // test can read it back via an ordinary D3D11 staging Map instead of `wgpu`'s broken
    // per-plane copy path.
    // SAFETY: CreateSharedHandle on a live, owned D3D12 resource.
    let shared_handle = unsafe {
        device12
            .CreateSharedHandle(&resource12, None, GENERIC_ALL.0, PCWSTR::null())
            .expect("ID3D12Device::CreateSharedHandle for readback")
    };
    let d3d11_device1: ID3D11Device1 = d3d11_device
        .cast()
        .expect("ID3D11Device1 cast for OpenSharedResource1");
    // SAFETY: opening the shared handle on the SAME adapter the bridge itself already
    // validated (D3d11SharedDecodeBridge::open's two-sided LUID check, exercised by the
    // successful bridge construction above).
    let opened_texture: ID3D11Texture2D = unsafe {
        d3d11_device1
            .OpenSharedResource1(shared_handle)
            .expect("ID3D11Device1::OpenSharedResource1 for readback")
    };
    // SAFETY: `shared_handle` is a raw NT handle from CreateSharedHandle, not a COM object —
    // closed once `opened_texture` holds its own independent D3D11 reference to the resource.
    let _ = unsafe { CloseHandle(shared_handle) };

    let read_staging = create_nv12_texture(
        &d3d11_device,
        width,
        height,
        D3D11_USAGE_STAGING,
        D3D11_CPU_ACCESS_READ.0 as u32,
        0,
    );
    let readback = read_nv12_via_staging(
        &d3d11_context,
        &read_staging,
        &opened_texture,
        width,
        height,
    );

    assert_eq!(
        readback, pixels,
        "NV12 byte-exact round trip mismatch: D3D11 write -> copy_from_decoded -> \
         import_decoded_texture -> D3D12->D3D11 readback"
    );

    eprintln!(
        "wgpu dx12 decode bridge: byte-exact pixel round trip ok ({} bytes: {}x{} NV12)",
        readback.len(),
        width,
        height
    );
}
