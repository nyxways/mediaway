//! Integration: DX11 Zero-Copy H.264 encode → fMP4 mux → demux (skip without HW MFT).

#![cfg(windows)]
#![allow(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "integration test"
)]

use mediaway_common::{
    Bytes, CodecKind, GpuBufferHandle, GpuDeviceHandle, NativeHandle, Packet, PixelFormat,
    Rational, VideoFrame, VideoFrameStorage,
};
use mediaway_container::mp4::{Demuxer, Muxer};
use mediaway_encoder::{VideoEncoder, VideoEncoderConfig, VideoInputPreference};
use mediaway_encoder_windows::WindowsVideoEncoder;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11CreateDevice, ID3D11Device, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
use windows::core::Interface;

fn drain_packets(enc: &mut WindowsVideoEncoder) -> Vec<Packet> {
    let mut out = Vec::new();
    while let Some(p) = enc.poll_packet().expect("poll") {
        out.push(p);
    }
    out
}

#[test]
fn av_fmp4_dx11_zero_copy_roundtrip() {
    let mut device: Option<ID3D11Device> = None;
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
    let Some(device) = device else {
        eprintln!("skip: D3D11CreateDevice failed ({hr:?})");
        return;
    };
    let device_handle =
        NativeHandle::new(Interface::as_raw(&device) as usize).expect("device pointer");

    let vcfg = VideoEncoderConfig {
        codec: CodecKind::H264,
        width: 64,
        height: 64,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        color_range: mediaway_common::ColorRange::Video,
        input: VideoInputPreference::ZeroCopyGpu,
        gpu_device: Some(GpuDeviceHandle::DirectX11(device_handle)),
        gop_size: 1,
        rate_control: None,
    };
    let mut venc = match WindowsVideoEncoder::open(&vcfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: no HW H.264 DXGI encode ({e:?})");
            return;
        }
    };

    let desc = D3D11_TEXTURE2D_DESC {
        Width: 64,
        Height: 64,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_NV12,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut texture: Option<ID3D11Texture2D> = None;
    if unsafe { device.CreateTexture2D(&raw const desc, None, Some(&raw mut texture)) }.is_err() {
        eprintln!("skip: CreateTexture2D NV12 failed");
        return;
    }
    let texture = texture.expect("texture");
    let tex_handle =
        NativeHandle::new(Interface::as_raw(&texture) as usize).expect("texture pointer");

    for i in 0..3u64 {
        let frame = VideoFrame {
            pts: i64::try_from(i).expect("pts"),
            duration: 1,
            width: 64,
            height: 64,
            format: PixelFormat::Nv12,
            storage: VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 {
                texture: tex_handle,
                subresource: 0,
            }),
        };
        venc.push_frame(&frame).expect("zc push");
    }
    venc.flush().expect("flush");
    let mut vpackets = drain_packets(&mut venc);
    assert!(!vpackets.is_empty(), "expected Zero-Copy H.264 packets");

    let vinfo = venc.stream_info().clone().with_id(0);

    let mut open = Muxer::with_fragment_batch(2);
    open.add_track(vinfo).expect("video track");
    let mut mux = open.begin();
    for p in &mut vpackets {
        p.stream_id = 0;
        mux.push_packet(p).expect("mux");
    }
    mux.flush();

    let mut bytes = Vec::new();
    mux.poll_bytes(&mut bytes);
    assert!(bytes.len() > 12);
    assert_eq!(&bytes[4..8], b"ftyp");

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    assert_eq!(demux.streams().len(), 1);
    let mut demuxed = 0usize;
    while demux.poll_packet().is_some() {
        demuxed += 1;
    }
    assert!(demuxed >= vpackets.len(), "demuxed {demuxed}");
    let _ = Bytes::new();
}
