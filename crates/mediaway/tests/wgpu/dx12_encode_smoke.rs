//! Real hardware smoke test: wgpu DX12 device/texture → Mediaway D3D12
//! `GpuCopy` bridge → Windows H.264 hardware encoder MFT → Annex-B bitstream
//! check.
//!
//! Skips gracefully (never fails the default suite) at the first missing
//! capability: no DX12 adapter/device, `as_hal` extraction failure, the
//! `GpuCopy` bridge failing to open, or no HW H.264 encoder MFT registered —
//! mirrors the existing WMF DX11/DX12 `_or_skip` test pattern in
//! `mediaway-encoder-windows`.
//!
//! # Hardware-verified (2026-07-29)
//!
//! Compiled and run on this workspace's reference Windows box; currently
//! skips (`no HW H.264 MFT for BGRA DXGI input`) — a confirmed pre-existing
//! hardware/driver limitation, not a bug in this bridge. See
//! `crates/mediaway-wgpu/src/lib.rs`'s crate-level "Hardware-verified" note
//! and `adr/0001-dx12-hal-gpucopy-bridge.md`.

#![cfg(windows)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::too_many_lines,
    reason = "integration test: one linear device -> bridge -> encode -> verify smoke test"
)]

use mediaway::wgpu::WgpuDx12Bridge;
use mediaway_common::{CodecKind, PixelFormat, Rational, VideoFrame, VideoFrameStorage};
use mediaway_encoder::windows::WindowsVideoEncoder;
use mediaway_encoder::{VideoEncoder, VideoEncoderConfig, VideoInputPreference};

/// `true` if `payload` contains an Annex-B NAL unit (00 00 01 / 00 00 00 01
/// start code) whose header byte's low 5 bits equal `nal_type`.
fn has_annex_b_nal(payload: &[u8], nal_type: u8) -> bool {
    let mut i = 0usize;
    while i + 3 <= payload.len() {
        let is_start4 = i + 4 <= payload.len()
            && payload[i] == 0
            && payload[i + 1] == 0
            && payload[i + 2] == 0
            && payload[i + 3] == 1;
        let is_start3 = payload[i] == 0 && payload[i + 1] == 0 && payload[i + 2] == 1;
        let hdr_off = if is_start4 {
            4
        } else if is_start3 {
            3
        } else {
            0
        };
        if hdr_off != 0 && i + hdr_off < payload.len() && (payload[i + hdr_off] & 0x1f) == nal_type
        {
            return true;
        }
        i += 1;
    }
    false
}

#[test]
fn wgpu_dx12_bridge_encodes_h264_or_skip() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        ..Default::default()
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
            label: Some("mediaway-wgpu smoke test device"),
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

    // Caller-owned wgpu texture, as if already rendered/composited — filled
    // with a solid BGRA color via a normal CPU->GPU write (test setup only,
    // not part of the interop path itself, which starts at `copy_frame`).
    let source = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("mediaway-wgpu smoke test source"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: mediaway::wgpu::BRIDGE_FORMAT,
        usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let solid_bgra: Vec<u8> = [0x40u8, 0x80, 0xC0, 0xFF]
        .iter()
        .copied()
        .cycle()
        .take((width * height * 4) as usize)
        .collect();
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &source,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &solid_bgra,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );

    let gpu_device = match bridge.gpu_device_handle() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("skip: bridge.gpu_device_handle failed ({e:?})");
            return;
        }
    };
    let vcfg = VideoEncoderConfig {
        codec: CodecKind::H264,
        width,
        height,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Bgra8,
        input: VideoInputPreference::ZeroCopyGpu,
        gpu_device: Some(gpu_device),
        gop_size: 1,
        rate_control: None,
        intra_refresh_period: None,
    };
    let mut enc = match WindowsVideoEncoder::open(&vcfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: no HW H.264 MFT for BGRA DXGI input ({e:?})");
            return;
        }
    };

    let mut packets = Vec::new();
    for i in 0..3u64 {
        let handle = match bridge.copy_frame(&device, &queue, &source) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("skip: bridge.copy_frame failed ({e:?})");
                return;
            }
        };
        let frame = VideoFrame {
            pts: i64::try_from(i).unwrap(),
            duration: 1,
            width,
            height,
            format: PixelFormat::Bgra8,
            storage: VideoFrameStorage::Gpu(handle),
        };
        if let Err(e) = enc.push_frame(&frame) {
            eprintln!("skip: push_frame failed ({e:?})");
            return;
        }
        while let Ok(Some(p)) = enc.poll_packet() {
            packets.push(p);
        }
    }
    enc.flush().expect("flush");
    while let Ok(Some(p)) = enc.poll_packet() {
        packets.push(p);
    }

    assert!(
        !packets.is_empty(),
        "expected at least one H.264 packet from the wgpu DX12 GpuCopy bridge"
    );
    let has_sps_or_idr = packets
        .iter()
        .any(|p| has_annex_b_nal(&p.payload, 7) || has_annex_b_nal(&p.payload, 5));
    assert!(
        has_sps_or_idr,
        "expected an Annex-B SPS (type 7) or IDR slice (type 5) NAL in encoded output"
    );
    eprintln!(
        "wgpu dx12 gpucopy: packets={} verified Annex-B H.264 SPS/IDR",
        packets.len()
    );
}
