//! Hardware tests for the live NVENC session: skip (pass) without an NVIDIA GPU.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::panic,
    clippy::cast_possible_truncation,
    reason = "test modules may unwrap / print"
)]

use super::super::{IntraRefresh, NvencLiveConfig, NvencLiveEncoder, NvencLiveInput};
use crate::{EncodeError, VideoEncoder};
use mediaway_common::{
    GpuBufferHandle, GpuDeviceHandle, NativeHandle, Packet, PixelFormat, VideoFrame,
    VideoFrameStorage,
};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11CreateDevice, ID3D11Device,
    ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1};
use windows::core::Interface;

const NVIDIA: u32 = 0x10DE;

fn nvidia_device() -> Option<(ID3D11Device, ID3D11DeviceContext)> {
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;
    let adapter: IDXGIAdapter1 = (0..)
        .map_while(|i| unsafe { factory.EnumAdapters1(i) }.ok())
        .find(|a| unsafe { a.GetDesc1() }.is_ok_and(|d| d.VendorId == NVIDIA))?;
    let mut device = None;
    let mut ctx = None;
    unsafe {
        D3D11CreateDevice(
            &adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&raw mut device),
            None,
            Some(&raw mut ctx),
        )
    }
    .ok()?;
    Some((device?, ctx?))
}

fn texture(device: &ID3D11Device, w: u32, h: u32, format: DXGI_FORMAT) -> ID3D11Texture2D {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: w,
        Height: h,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut tex = None;
    unsafe { device.CreateTexture2D(&raw const desc, None, Some(&raw mut tex)) }.unwrap();
    tex.unwrap()
}

/// A moving gradient so P-frames have something to code.
fn fill(ctx: &ID3D11DeviceContext, tex: &ID3D11Texture2D, w: u32, h: u32, frame: u32) {
    let mut px = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            px[i] = ((x + frame * 4) & 0xFF) as u8;
            px[i + 1] = ((y + frame * 2) & 0xFF) as u8;
            px[i + 2] = ((x ^ y) & 0xFF) as u8;
            px[i + 3] = 0xFF;
        }
    }
    unsafe { ctx.UpdateSubresource(tex, 0, None, px.as_ptr().cast(), w * 4, 0) };
}

fn frame(tex: &ID3D11Texture2D, w: u32, h: u32, pts: i64) -> VideoFrame {
    VideoFrame {
        pts,
        duration: 1,
        width: w,
        height: h,
        format: PixelFormat::Bgra8,
        storage: VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 {
            texture: NativeHandle::new(tex.as_raw() as usize).unwrap(),
            subresource: 0,
        }),
    }
}

fn nal_types(annex_b: &[u8]) -> Vec<u8> {
    annex_b
        .windows(4)
        .enumerate()
        .filter(|(_, w)| w[..3] == [0, 0, 1])
        .map(|(i, _)| annex_b[i + 3] & 0x1F)
        .collect()
}

fn device_handle(device: &ID3D11Device) -> GpuDeviceHandle {
    GpuDeviceHandle::DirectX11(NativeHandle::new(device.as_raw() as usize).unwrap())
}

fn one_packet(enc: &mut NvencLiveEncoder) -> Packet {
    let p = enc.poll_packet().unwrap().expect("one packet per frame");
    assert!(
        enc.poll_packet().unwrap().is_none(),
        "exactly one packet per frame"
    );
    p
}

#[test]
fn live_h264_keyframe_on_demand_bitrate_and_resize_or_skip_without_hw() {
    let Some((device, ctx)) = nvidia_device() else {
        eprintln!("skip: no NVIDIA D3D11 adapter");
        return;
    };
    let mut cfg = NvencLiveConfig::new(device_handle(&device), 640, 360, 60, 1, 2_000_000);
    cfg.max_width = 1280;
    cfg.max_height = 720;
    let mut enc = match NvencLiveEncoder::open(&cfg) {
        Ok(e) => e,
        Err(EncodeError::Backend | EncodeError::Unsupported) => {
            eprintln!("skip: NVENC (API 13.0) unavailable");
            return;
        }
        Err(e) => panic!("open: {e:?}"),
    };
    // Two textures alternate, like a capture frame pool.
    let texs = [
        texture(&device, 640, 360, DXGI_FORMAT_B8G8R8A8_UNORM),
        texture(&device, 640, 360, DXGI_FORMAT_B8G8R8A8_UNORM),
    ];
    for i in 0..40u32 {
        let tex = &texs[(i % 2) as usize];
        fill(&ctx, tex, 640, 360, i);
        if i == 20 {
            enc.request_keyframe();
        }
        if i == 25 {
            enc.set_bitrate(1_000_000).unwrap();
            assert_eq!(enc.bitrate(), 1_000_000);
        }
        enc.push_frame(&frame(tex, 640, 360, i64::from(i))).unwrap();
        let p = one_packet(&mut enc);
        assert_eq!(p.pts, i64::from(i));
        let nals = nal_types(&p.payload);
        let key = i == 0 || i == 20;
        assert_eq!(p.is_keyframe, key, "frame {i}: nals {nals:?}");
        if key {
            assert!(
                nals.contains(&7) && nals.contains(&8) && nals.contains(&5),
                "{nals:?}"
            );
        } else {
            assert!(
                !nals.contains(&7) && !nals.contains(&5) && nals.contains(&1),
                "{nals:?}"
            );
        }
    }

    enc.resize(1280, 720).unwrap();
    // A texture of the old size is now too small.
    assert_eq!(
        enc.push_frame(&frame(&texs[0], 640, 360, 40)).err(),
        Some(EncodeError::InvalidInput)
    );
    let big = texture(&device, 1280, 720, DXGI_FORMAT_B8G8R8A8_UNORM);
    fill(&ctx, &big, 1280, 720, 41);
    enc.push_frame(&frame(&big, 1280, 720, 41)).unwrap();
    let p = one_packet(&mut enc);
    assert!(p.is_keyframe);
    assert!(nal_types(&p.payload).contains(&7));
    assert_eq!(
        enc.resize(1920, 1080).err(),
        Some(EncodeError::InvalidInput)
    );

    enc.flush().unwrap();
    assert_eq!(
        enc.push_frame(&frame(&big, 1280, 720, 42)).err(),
        Some(EncodeError::Closed)
    );
}

#[test]
fn live_h264_intra_refresh_has_no_periodic_idr_or_skip_without_hw() {
    let Some((device, ctx)) = nvidia_device() else {
        eprintln!("skip: no NVIDIA D3D11 adapter");
        return;
    };
    let mut cfg = NvencLiveConfig::new(device_handle(&device), 640, 360, 60, 1, 2_000_000);
    cfg.intra_refresh = Some(IntraRefresh {
        period_frames: 10,
        duration_frames: 5,
    });
    let Ok(mut enc) = NvencLiveEncoder::open(&cfg) else {
        eprintln!("skip: NVENC (API 13.0) or intra refresh unavailable");
        return;
    };
    let tex = texture(&device, 640, 360, DXGI_FORMAT_B8G8R8A8_UNORM);
    let mut recovery_points = 0;
    for i in 0..40u32 {
        fill(&ctx, &tex, 640, 360, i);
        enc.push_frame(&frame(&tex, 640, 360, i64::from(i)))
            .unwrap();
        let p = one_packet(&mut enc);
        assert_eq!(p.is_keyframe, i == 0, "frame {i}");
        if i > 0 && nal_types(&p.payload).contains(&6) {
            recovery_points += 1;
        }
    }
    assert!(
        recovery_points >= 3,
        "recovery-point SEI count {recovery_points}"
    );
}

#[test]
fn live_h264_rejects_a_texture_of_the_wrong_format_or_skip_without_hw() {
    let Some((device, _ctx)) = nvidia_device() else {
        eprintln!("skip: no NVIDIA D3D11 adapter");
        return;
    };
    let cfg = NvencLiveConfig::new(device_handle(&device), 640, 360, 60, 1, 2_000_000);
    assert_eq!(cfg.input, NvencLiveInput::Bgra);
    let Ok(mut enc) = NvencLiveEncoder::open(&cfg) else {
        eprintln!("skip: NVENC (API 13.0) unavailable");
        return;
    };
    let rgba = texture(&device, 640, 360, DXGI_FORMAT_R8G8B8A8_UNORM);
    assert_eq!(
        enc.push_frame(&frame(&rgba, 640, 360, 0)).err(),
        Some(EncodeError::InvalidInput)
    );
    // CPU frames belong to `NvencVideoEncoder`.
    let cpu = VideoFrame {
        pts: 0,
        duration: 1,
        width: 640,
        height: 360,
        format: PixelFormat::Nv12,
        storage: VideoFrameStorage::Cpu {
            data: mediaway_common::Bytes::from(vec![0u8; 640 * 360 * 3 / 2]),
        },
    };
    assert_eq!(enc.push_frame(&cpu).err(), Some(EncodeError::Unsupported));
}
