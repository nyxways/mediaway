//! Live H.264 round trip on real hardware: `NvencLiveEncoder` (D3D11 BGRA textures in) →
//! `WmfLiveH264Decoder` (DXVA, D3D11 NV12 out + readback). Skips (passes) without an NVIDIA
//! GPU, NVENC API 13.0, or a DXVA H.264 decoder.
//!
//! The latency measurement is `#[ignore]`d (it paces 600 frames at 60 fps, ~10 s):
//!
//! ```text
//! cargo nextest run -p mediaway-decoder --test live_h264_roundtrip --run-ignored all \
//!     -E 'test(live_latency)' --no-capture
//! ```
#![cfg(windows)]
#![allow(
    unsafe_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::many_single_char_names,
    clippy::suboptimal_flops,
    reason = "hardware integration test"
)]

use std::time::{Duration, Instant};

use mediaway_common::{GpuDeviceHandle, NativeHandle, Packet, VideoFrameStorage};
use mediaway_decoder::windows::{WmfLiveH264Config, WmfLiveH264Decoder};
use mediaway_decoder::{DecodeError, VideoDecoder};
use mediaway_encoder::nvenc::{NvencLiveConfig, NvencLiveEncoder};
use mediaway_encoder::{EncodeError, VideoEncoder};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_QUERY_DESC, D3D11_QUERY_EVENT, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11CreateDevice, ID3D11Device,
    ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1};
use windows::core::{BOOL, Interface};

const W: u32 = 1920;
const H: u32 = 1080;

struct Gpu {
    device: ID3D11Device,
    ctx: ID3D11DeviceContext,
}

impl Gpu {
    fn nvidia() -> Option<Self> {
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;
        let adapter: IDXGIAdapter1 = (0..)
            .map_while(|i| unsafe { factory.EnumAdapters1(i) }.ok())
            .find(|a| unsafe { a.GetDesc1() }.is_ok_and(|d| d.VendorId == 0x10DE))?;
        let mut device = None;
        let mut ctx = None;
        unsafe {
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&raw mut device),
                None,
                Some(&raw mut ctx),
            )
        }
        .ok()?;
        Some(Self {
            device: device?,
            ctx: ctx?,
        })
    }

    fn handle(&self) -> GpuDeviceHandle {
        GpuDeviceHandle::DirectX11(NativeHandle::new(self.device.as_raw() as usize).unwrap())
    }

    fn bgra_texture(&self, w: u32, h: u32) -> ID3D11Texture2D {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
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
        unsafe {
            self.device
                .CreateTexture2D(&raw const desc, None, Some(&raw mut tex))
        }
        .unwrap();
        tex.unwrap()
    }

    fn upload(&self, tex: &ID3D11Texture2D, bgra: &[u8], w: u32) {
        unsafe {
            self.ctx
                .UpdateSubresource(tex, 0, None, bgra.as_ptr().cast(), w * 4, 0);
        }
    }

    /// Block until the GPU finished everything submitted on this device so far.
    fn wait_idle(&self) {
        let desc = D3D11_QUERY_DESC {
            Query: D3D11_QUERY_EVENT,
            MiscFlags: 0,
        };
        let mut query = None;
        unsafe {
            self.device
                .CreateQuery(&raw const desc, Some(&raw mut query))
        }
        .unwrap();
        let query = query.unwrap();
        unsafe { self.ctx.End(&query) };
        loop {
            let mut done = BOOL(0);
            let _ = unsafe {
                self.ctx.GetData(
                    &query,
                    Some((&raw mut done).cast()),
                    std::mem::size_of::<BOOL>() as u32,
                    0,
                )
            };
            if done.as_bool() {
                return;
            }
            std::hint::spin_loop();
        }
    }
}

/// Screen-like test picture: smooth gradients, a moving bright block and fine stripes.
fn picture(w: u32, h: u32, n: u32, out: &mut [u8]) {
    let bx = (n * 13) % (w - 200);
    let by = (n * 7) % (h - 120);
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let in_block = x >= bx && x < bx + 200 && y >= by && y < by + 120;
            let stripe = if (x / 4 + y / 4) % 2 == 0 { 24 } else { 0 };
            let (r, g, b) = if in_block {
                (240, 200, 40)
            } else {
                (
                    (x * 255 / w) as u8,
                    (y * 255 / h) as u8,
                    (((x + y + n * 3) % 256) as u8 / 2).saturating_add(stripe),
                )
            };
            out[i] = b;
            out[i + 1] = g;
            out[i + 2] = r;
            out[i + 3] = 255;
        }
    }
}

/// Luma PSNR (dB) of `y` against `bgra` converted with `(kr, kb)` limited range.
fn luma_psnr(bgra: &[u8], y: &[u8], w: u32, h: u32, kr: f64, kb: f64) -> f64 {
    let kg = 1.0 - kr - kb;
    let mut se = 0.0f64;
    for i in 0..(w * h) as usize {
        let (b, g, r) = (
            f64::from(bgra[i * 4]),
            f64::from(bgra[i * 4 + 1]),
            f64::from(bgra[i * 4 + 2]),
        );
        let reference = 16.0 + 219.0 * (kr * r + kg * g + kb * b) / 255.0;
        let d = reference - f64::from(y[i]);
        se += d * d;
    }
    let mse = se / f64::from(w * h);
    if mse == 0.0 {
        99.0
    } else {
        10.0 * (255.0f64 * 255.0 / mse).log10()
    }
}

const BT709: (f64, f64) = (0.2126, 0.0722);
const BT601: (f64, f64) = (0.299, 0.114);

struct Rig {
    tx: Gpu,
    rx: Gpu,
    enc: NvencLiveEncoder,
    dec: WmfLiveH264Decoder,
}

fn rig(bitrate: u32) -> Option<Rig> {
    // Sender and receiver on separate devices, like two machines.
    let (Some(tx), Some(rx)) = (Gpu::nvidia(), Gpu::nvidia()) else {
        eprintln!("skip: no NVIDIA D3D11 adapter");
        return None;
    };
    let cfg = NvencLiveConfig::new(tx.handle(), W, H, 60, 1, bitrate);
    let enc = match NvencLiveEncoder::open(&cfg) {
        Ok(e) => e,
        Err(EncodeError::Backend | EncodeError::Unsupported) => {
            eprintln!("skip: NVENC (API 13.0) unavailable");
            return None;
        }
        Err(e) => panic!("encoder open: {e:?}"),
    };
    let dec = match WmfLiveH264Decoder::open(&WmfLiveH264Config::new(rx.handle(), W, H)) {
        Ok(d) => d,
        Err(DecodeError::NoHardwareDecoder) => {
            eprintln!("skip: no DXVA H.264 decoder MFT");
            return None;
        }
        Err(e) => panic!("decoder open: {e:?}"),
    };
    eprintln!("decoder MFT: {}", dec.decoder_name());
    Some(Rig { tx, rx, enc, dec })
}

fn encode(rig: &mut Rig, tex: &ID3D11Texture2D, w: u32, n: u32, bgra: &[u8]) -> Packet {
    rig.tx.upload(tex, bgra, w);
    rig.enc
        .encode_d3d11(
            NativeHandle::new(tex.as_raw() as usize).unwrap(),
            0,
            i64::from(n),
            1,
        )
        .unwrap();
    rig.enc.poll_packet().unwrap().unwrap()
}

#[test]
fn live_nvenc_to_wmf_dxva_roundtrip_with_loss_and_resize_or_skip_without_hw() {
    let Some(mut rig) = rig(10_000_000) else {
        return;
    };
    let tex = rig.tx.bgra_texture(W, H);
    let mut bgra = vec![0u8; (W * H * 4) as usize];
    let mut y = vec![0u8; (W * H) as usize];
    let mut uv = vec![0u8; (W * H / 2) as usize];
    let mut decoded = 0;
    let mut psnr_709_min = f64::MAX;
    let mut psnr_601_max = 0.0f64;
    for n in 0..130u32 {
        picture(W, H, n, &mut bgra);
        if n == 40 {
            rig.enc.set_bitrate(6_000_000).unwrap();
        }
        if n == 60 || n == 100 {
            rig.enc.request_keyframe();
        }
        let packet = encode(&mut rig, &tex, W, n, &bgra);
        assert_eq!(
            packet.is_keyframe,
            n == 0 || n == 60 || n == 100,
            "frame {n}"
        );
        // Loss without reset (20..=22): the decoder conceals and keeps going.
        // Loss with reset (80..=84): frames are held back until the IDR at 100.
        if (20..=22).contains(&n) || (80..=84).contains(&n) {
            if n == 84 {
                rig.dec.reset().unwrap();
                assert!(rig.dec.is_waiting_for_keyframe());
            }
            continue;
        }
        rig.dec.push_packet(&packet).unwrap();
        let frame = rig.dec.poll_frame().unwrap();
        if (85..100).contains(&n) {
            assert!(frame.is_none(), "frame {n} must wait for the IDR");
            continue;
        }
        // Low latency: the frame for packet n comes out right after packet n.
        let frame = frame.unwrap_or_else(|| panic!("no frame right after packet {n}"));
        assert!(rig.dec.poll_frame().unwrap().is_none());
        assert_eq!((frame.width, frame.height), (W, H), "frame {n}");
        assert_eq!(frame.pts, i64::from(n));
        assert!(matches!(frame.storage, VideoFrameStorage::Gpu(_)));
        decoded += 1;
    }
    assert_eq!(rig.dec.dropped_packets(), 15);
    assert_eq!(decoded, 130 - 3 - 5 - 15);

    // Readback + content check on clean frames after the last IDR.
    for n in 130..140u32 {
        picture(W, H, n, &mut bgra);
        let packet = encode(&mut rig, &tex, W, n, &bgra);
        rig.dec.push_packet(&packet).unwrap();
        let frame = rig.dec.poll_frame().unwrap().unwrap();
        let size = rig
            .dec
            .read_nv12(&mut y, W as usize, &mut uv, W as usize)
            .unwrap();
        assert_eq!(size, (frame.width, frame.height));
        let p709 = luma_psnr(&bgra, &y, W, H, BT709.0, BT709.1);
        let p601 = luma_psnr(&bgra, &y, W, H, BT601.0, BT601.1);
        psnr_709_min = psnr_709_min.min(p709);
        psnr_601_max = psnr_601_max.max(p601);
    }
    eprintln!(
        "luma PSNR vs BT.709 reference: min {psnr_709_min:.1} dB; vs BT.601: max {psnr_601_max:.1} dB"
    );
    assert!(psnr_709_min > 30.0, "luma PSNR {psnr_709_min:.1} dB");

    // Mid-stream resolution change (the shared window shrank): new SPS, the decoder follows.
    let (w2, h2) = (1280u32, 720u32);
    rig.enc.resize(w2, h2).unwrap();
    let small = rig.tx.bgra_texture(w2, h2);
    let mut bgra2 = vec![0u8; (w2 * h2 * 4) as usize];
    for n in 140..150u32 {
        picture(w2, h2, n, &mut bgra2);
        let packet = encode(&mut rig, &small, w2, n, &bgra2);
        assert_eq!(packet.is_keyframe, n == 140);
        rig.dec.push_packet(&packet).unwrap();
        let frame = rig.dec.poll_frame().unwrap().unwrap();
        assert_eq!((frame.width, frame.height), (w2, h2), "frame {n}");
        rig.dec
            .read_nv12(&mut y, w2 as usize, &mut uv, w2 as usize)
            .unwrap();
        let p = luma_psnr(&bgra2, &y, w2, h2, BT709.0, BT709.1);
        assert!(p > 30.0, "720p frame {n}: luma PSNR {p:.1} dB");
    }
    rig.enc.flush().unwrap();
    rig.dec.flush().unwrap();
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

fn report(label: &str, mut ms: Vec<f64>) {
    ms.sort_by(f64::total_cmp);
    eprintln!(
        "{label}: median {:.2} ms, p95 {:.2} ms, max {:.2} ms (n = {})",
        percentile(&ms, 0.5),
        percentile(&ms, 0.95),
        ms[ms.len() - 1],
        ms.len()
    );
}

/// Per-frame latency at 1080p60, 10 Mbps, paced at 60 fps (one frame every 16.7 ms, like a
/// capture loop). The first 60 frames are warm-up and not counted.
#[test]
#[ignore = "measurement: paces 600 frames at 60 fps (~10 s); run with --run-ignored all"]
fn live_latency_1080p60() {
    let Some(mut rig) = rig(10_000_000) else {
        return;
    };
    let texs = [rig.tx.bgra_texture(W, H), rig.tx.bgra_texture(W, H)];
    // Pre-render a few pictures so CPU drawing is not inside the paced loop.
    let pictures: Vec<Vec<u8>> = (0..8)
        .map(|n| {
            let mut p = vec![0u8; (W * H * 4) as usize];
            picture(W, H, n * 17, &mut p);
            p
        })
        .collect();
    let mut y = vec![0u8; (W * H) as usize];
    let mut uv = vec![0u8; (W * H / 2) as usize];
    let (mut enc_ms, mut dec_submit_ms, mut dec_gpu_ms, mut readback_ms, mut bytes) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), 0usize);
    let frame_time = Duration::from_micros(16_667);
    let start = Instant::now();
    for n in 0..600u32 {
        let deadline = start + frame_time * n;
        if let Some(wait) = deadline.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
        let tex = &texs[(n % 2) as usize];
        rig.tx.upload(tex, &pictures[(n % 8) as usize], W);
        rig.tx.wait_idle();
        let handle = NativeHandle::new(tex.as_raw() as usize).unwrap();

        let t0 = Instant::now();
        rig.enc.encode_d3d11(handle, 0, i64::from(n), 1).unwrap();
        let packet = rig.enc.poll_packet().unwrap().unwrap();
        let t1 = Instant::now();
        rig.dec.push_packet(&packet).unwrap();
        let frame = rig.dec.poll_frame().unwrap();
        let t2 = Instant::now();
        rig.rx.wait_idle();
        let t3 = Instant::now();
        assert!(frame.is_some(), "frame {n}");
        rig.dec
            .read_nv12(&mut y, W as usize, &mut uv, W as usize)
            .unwrap();
        let t4 = Instant::now();
        if n >= 60 {
            let ms = |a: Instant, b: Instant| (b - a).as_secs_f64() * 1000.0;
            enc_ms.push(ms(t0, t1));
            dec_submit_ms.push(ms(t1, t2));
            dec_gpu_ms.push(ms(t1, t3));
            readback_ms.push(ms(t1, t4));
            bytes += packet.payload.len();
        }
    }
    let frames = enc_ms.len();
    eprintln!(
        "decoder MFT: {}; average packet {} bytes ({:.2} Mbit/s at 60 fps)",
        rig.dec.decoder_name(),
        bytes / frames,
        (bytes * 8 * 60) as f64 / frames as f64 / 1e6
    );
    report("encode (encode_d3d11 -> packet)", enc_ms);
    report("decode submit (push_packet -> poll_frame)", dec_submit_ms);
    report(
        "decode done on GPU (push_packet -> event query)",
        dec_gpu_ms,
    );
    report(
        "decode + NV12 readback (push_packet -> read_nv12)",
        readback_ms,
    );
}
