//! H.264 encode benches: DX11 Zero-Copy hardware path vs the inbox software (CPU) path.
//!
//! Path classes (see `docs/conventions/benchmarking.md`):
//! - `zc_wmf_h264_dx11` — hardware H.264 encoder MFT fed a DXGI NV12 surface directly
//!   (`MFCreateDXGISurfaceBuffer`, see `src/wmf/dx11.rs::sample_from_dx11_texture`): no
//!   payload memcpy on the Mediaway side, Zero-Copy.
//! - `sw_wmf_h264_cpu` — `VideoInputPreference::CpuUploadOk` for H.264 hardcodes the
//!   inbox **software** encoder CLSID (`CLSID_MSH264EncoderMFT`, see
//!   `src/wmf/video.rs::open_cpu`). This crate has no "CPU-uploaded NV12 sample fed to
//!   a hardware H.264 encoder MFT" path today, so the honest label is `sw`, not `copy`.
//!
//! Criterion performs its own warmup + many-sample measurement per `Fair measurement`
//! (`docs/conventions/benchmarking.md`); the encoder session is opened once per bench
//! function (outside the timed closure) and never flushed mid-run, so each timed
//! iteration is one steady-state `push_frame` + drain of an already-open session.
//! Frame content is a static mid-gray NV12 buffer/texture — content is irrelevant to
//! the push+drain throughput measured here (same convention as
//! `tests/av_fmp4_zc_smoke.rs` / `src/lib.rs` unit tests).
//!
//! Everything below lives in `imp`, gated to `windows` + the `video` feature (the DX11
//! adapter enumeration reaches for the `windows` crate directly, which isn't a
//! dependency off-Windows) — kept out of a whole-file `#![cfg(...)]` so this target
//! still has a real `fn main` (and so still compiles) on every other platform, matching
//! `harness = false`'s requirement that the bench binary provide its own entry point.

#![allow(
    missing_docs,
    reason = "bench harness; criterion_group! expands an undocumented fn"
)]

#[cfg(all(windows, feature = "video"))]
mod imp {
    #![allow(unsafe_code)]
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::print_stderr,
        reason = "bench harness, not shipped product code"
    )]

    use criterion::{Criterion, criterion_group};
    use mediaway_common::{
        Bytes, CodecKind, GpuBufferHandle, GpuDeviceHandle, NativeHandle, PixelFormat, Rational,
        VideoFrame, VideoFrameStorage,
    };
    use mediaway_encoder::{VideoEncoder, VideoEncoderConfig, VideoInputPreference};
    use mediaway_encoder_windows::WindowsVideoEncoder;
    use std::hint::black_box;

    const WIDTH: u32 = 640;
    const HEIGHT: u32 = 480;
    const BITRATE_BPS: u32 = 4_000_000;

    const fn nv12_len() -> usize {
        (WIDTH * HEIGHT + WIDTH * HEIGHT / 2) as usize
    }

    /// Static mid-gray NV12 CPU frame. Content doesn't matter, only that it encodes.
    fn cpu_frame(pts: i64) -> VideoFrame {
        VideoFrame {
            pts,
            duration: 1,
            width: WIDTH,
            height: HEIGHT,
            format: PixelFormat::Nv12,
            storage: VideoFrameStorage::Cpu {
                data: Bytes::from(vec![128u8; nv12_len()]),
            },
        }
    }

    fn bench_sw_wmf_h264_cpu(c: &mut Criterion) {
        let cfg = VideoEncoderConfig {
            codec: CodecKind::H264,
            width: WIDTH,
            height: HEIGHT,
            time_base: Rational::new(1, 30),
            bitrate_bps: BITRATE_BPS,
            pixel_format: PixelFormat::Nv12,
            color_range: mediaway_common::ColorRange::Video,
            input: VideoInputPreference::CpuUploadOk,
            gpu_device: None,
            gop_size: 1,
            rate_control: None,
        };
        let mut enc = match WindowsVideoEncoder::open(&cfg) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("skip sw_wmf_h264_cpu: WindowsVideoEncoder::open failed ({e:?})");
                return;
            }
        };

        let mut pts = 0i64;
        let mut group = c.benchmark_group("encode");
        group.bench_function("sw_wmf_h264_cpu", |b| {
            b.iter(|| {
                let frame = cpu_frame(pts);
                pts += 1;
                enc.push_frame(&frame).expect("push_frame");
                while let Some(p) = enc.poll_packet().expect("poll_packet") {
                    black_box(p);
                }
            });
        });
        group.finish();
    }

    fn bench_zc_wmf_h264_dx11(c: &mut Criterion) {
        use windows::Win32::Graphics::Direct3D11::{
            D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, ID3D11Texture2D,
        };
        use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
        use windows::core::Interface;

        // `D3D_DRIVER_TYPE_HARDWARE` with no adapter picks whichever adapter DXGI ranks
        // first (on this machine: the NVIDIA RTX 4090) — but NVIDIA does not register a
        // Media Foundation **encode** hardware transform (only decode), so that device
        // fails `WindowsVideoEncoder::open` with `EncodeError::Backend` even though the
        // adapter itself is real HW. Enumerate adapters and use the first one that
        // actually has a working HW H.264 encoder MFT (e.g. Intel Quick Sync) instead of
        // silently falling back to a software path under a `zc_` name.
        let Some((device, mut enc, adapter_name)) = open_encoder_on_any_adapter() else {
            eprintln!(
                "skip zc_wmf_h264_dx11: no adapter on this machine exposes a HW H.264 encoder MFT"
            );
            return;
        };
        eprintln!("zc_wmf_h264_dx11: HW adapter = {adapter_name}");

        let desc = D3D11_TEXTURE2D_DESC {
            Width: WIDTH,
            Height: HEIGHT,
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
        if unsafe { device.CreateTexture2D(&raw const desc, None, Some(&raw mut texture)) }.is_err()
        {
            eprintln!("skip zc_wmf_h264_dx11: CreateTexture2D NV12 failed");
            return;
        }
        let Some(texture) = texture else {
            eprintln!("skip zc_wmf_h264_dx11: null texture");
            return;
        };
        let Some(tex_handle) = NativeHandle::new(Interface::as_raw(&texture) as usize) else {
            eprintln!("skip zc_wmf_h264_dx11: null texture pointer");
            return;
        };

        let mut pts = 0i64;
        let mut group = c.benchmark_group("encode");
        group.bench_function("zc_wmf_h264_dx11", |b| {
            b.iter(|| {
                let frame = VideoFrame {
                    pts,
                    duration: 1,
                    width: WIDTH,
                    height: HEIGHT,
                    format: PixelFormat::Nv12,
                    storage: VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 {
                        texture: tex_handle,
                        subresource: 0,
                    }),
                };
                pts += 1;
                enc.push_frame(&frame).expect("push_frame");
                while let Some(p) = enc.poll_packet().expect("poll_packet") {
                    black_box(p);
                }
            });
        });
        group.finish();
    }

    /// Enumerate DXGI adapters and open a Zero-Copy H.264 encoder session on the first
    /// one that actually has a working hardware encoder MFT (NVIDIA GPUs typically only
    /// register a Media Foundation **decode** HW MFT, not encode — see the caller). Skips
    /// the WARP/basic-render software adapter; this bench is HW-only by construction.
    /// Returns the owning `ID3D11Device` (needed below to allocate the NV12 texture),
    /// the already-open encoder, and a friendly adapter name for the report.
    fn open_encoder_on_any_adapter() -> Option<(
        windows::Win32::Graphics::Direct3D11::ID3D11Device,
        WindowsVideoEncoder,
        String,
    )> {
        use windows::Win32::Foundation::HMODULE;
        use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
        use windows::Win32::Graphics::Direct3D11::{
            D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
        };
        use windows::Win32::Graphics::Dxgi::{
            CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
        };
        use windows::core::Interface;

        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;
        let mut index = 0u32;
        loop {
            let adapter = unsafe { factory.EnumAdapters1(index) }.ok()?;
            index += 1;
            let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
                continue;
            };
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            let end = desc
                .Description
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(desc.Description.len());
            let name = String::from_utf16_lossy(&desc.Description[..end]);

            let mut device: Option<ID3D11Device> = None;
            let hr = unsafe {
                D3D11CreateDevice(
                    &adapter,
                    D3D_DRIVER_TYPE_UNKNOWN,
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
                eprintln!("zc_wmf_h264_dx11: D3D11CreateDevice on {name} failed ({hr:?})");
                continue;
            };
            let Some(device_handle) = NativeHandle::new(Interface::as_raw(&device) as usize) else {
                continue;
            };
            let cfg = VideoEncoderConfig {
                codec: CodecKind::H264,
                width: WIDTH,
                height: HEIGHT,
                time_base: Rational::new(1, 30),
                bitrate_bps: BITRATE_BPS,
                pixel_format: PixelFormat::Nv12,
                color_range: mediaway_common::ColorRange::Video,
                input: VideoInputPreference::ZeroCopyGpu,
                gpu_device: Some(GpuDeviceHandle::DirectX11(device_handle)),
                gop_size: 1,
                rate_control: None,
            };
            match WindowsVideoEncoder::open(&cfg) {
                Ok(enc) => return Some((device, enc, name)),
                Err(e) => {
                    eprintln!(
                        "zc_wmf_h264_dx11: {name} has no working HW H.264 encoder MFT ({e:?})"
                    );
                }
            }
        }
    }

    criterion_group!(benches, bench_sw_wmf_h264_cpu, bench_zc_wmf_h264_dx11);
}

// `criterion_main!` must expand at crate root — it generates `fn main`, which
// `rustc` only looks for there, not inside `imp`.
#[cfg(all(windows, feature = "video"))]
criterion::criterion_main!(imp::benches);

/// Off-Windows / `video`-feature-disabled entry point — this bench has nothing to
/// measure there (see the module doc comment above).
#[cfg(not(all(windows, feature = "video")))]
fn main() {}
