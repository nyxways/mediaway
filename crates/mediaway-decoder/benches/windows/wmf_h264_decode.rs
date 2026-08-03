//! H.264 decode benches: DX11 Zero-Copy hardware output vs the CPU/software path.
//!
//! Path classes (see `docs/conventions/benchmarking.md`):
//! - `zc_wmf_h264_dx11` — hardware H.264 decoder MFT with DXGI output surfaces
//!   (`src/wmf/dx11.rs`): decoded frames stay GPU-resident, Zero-Copy.
//! - `sw_wmf_h264_cpu` — the synchronous software H.264 decoder MFT
//!   (`src/wmf/cpu.rs::open_sw_decoder`); no GPU device anywhere in the chain, so
//!   this is honest CPU decode, not a GPU→CPU readback (matches that module's own
//!   doc comment).
//!
//! Real compressed H.264 bytes are produced once per bench (untimed setup) by
//! encoding synthetic NV12 frames through `mediaway-encoder-windows`'s CPU path —
//! same approach as `tests/cpu_roundtrip.rs` — rather than any committed media file.
//! Each Criterion iteration opens a **fresh** decoder session via `iter_batched` (the
//! open call is untimed setup) and times only the steady push+drain+flush of that one
//! short sequence, since a decoder cannot be reused for a second sequence once
//! flushed (`DecodeError::Closed` after `flush`).
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

    use criterion::{BatchSize, Criterion, criterion_group};
    use mediaway_common::{
        Bytes, CodecKind, GpuDeviceHandle, NativeHandle, Packet, PixelFormat, Rational, VideoFrame,
        VideoFrameStorage,
    };
    use mediaway_decoder::{VideoDecoder, VideoDecoderConfig, VideoOutputPreference};
    use mediaway_decoder_windows::WindowsVideoDecoder;
    use mediaway_encoder::windows::WindowsVideoEncoder;
    use mediaway_encoder::{VideoEncoder, VideoEncoderConfig, VideoInputPreference};
    use std::hint::black_box;

    const WIDTH: u32 = 640;
    const HEIGHT: u32 = 480;
    const BITRATE_BPS: u32 = 4_000_000;
    /// Short GOP: enough to amortize per-batch decoder-open cost without a long encode step.
    const FRAME_COUNT: u32 = 30;

    /// Encode `FRAME_COUNT` static mid-gray NV12 frames via the CPU (software) H.264
    /// encoder MFT and return the real compressed packets + extradata, or `None` if MF
    /// / the inbox software encoder is unavailable on this machine.
    fn encode_h264_frames() -> Option<(Vec<Packet>, Bytes)> {
        let cfg = VideoEncoderConfig {
            codec: CodecKind::H264,
            width: WIDTH,
            height: HEIGHT,
            time_base: Rational::new(1, 30),
            bitrate_bps: BITRATE_BPS,
            pixel_format: PixelFormat::Nv12,
            input: VideoInputPreference::CpuUploadOk,
            gpu_device: None,
        };
        let mut enc = WindowsVideoEncoder::open(&cfg).ok()?;
        let nv12_len = (WIDTH * HEIGHT + WIDTH * HEIGHT / 2) as usize;
        let mut packets = Vec::new();
        for i in 0..FRAME_COUNT {
            let frame = VideoFrame {
                pts: i64::from(i),
                duration: 1,
                width: WIDTH,
                height: HEIGHT,
                format: PixelFormat::Nv12,
                storage: VideoFrameStorage::Cpu {
                    data: Bytes::from(vec![128u8; nv12_len]),
                },
            };
            enc.push_frame(&frame).expect("encoder push_frame");
            while let Some(p) = enc.poll_packet().expect("encoder poll_packet") {
                packets.push(p);
            }
        }
        enc.flush().expect("encoder flush");
        while let Some(p) = enc.poll_packet().expect("encoder poll_packet") {
            packets.push(p);
        }
        if packets.is_empty() {
            return None;
        }
        let extra_data = enc.stream_info().extra_data().clone(); // clone: owned snapshot outlives `enc`, used to build decoder configs below
        Some((packets, extra_data))
    }

    fn bench_sw_wmf_h264_cpu(c: &mut Criterion) {
        let Some((packets, extra_data)) = encode_h264_frames() else {
            eprintln!("skip sw_wmf_h264_cpu decode: CPU H.264 encode setup failed");
            return;
        };

        let cfg = VideoDecoderConfig {
            codec: CodecKind::H264,
            width: WIDTH,
            height: HEIGHT,
            time_base: Rational::new(1, 30),
            pixel_format: PixelFormat::Nv12,
            output: VideoOutputPreference::CpuFramesOk,
            gpu_device: None,
            extra_data,
        };
        if let Err(e) = WindowsVideoDecoder::open(&cfg) {
            eprintln!("skip sw_wmf_h264_cpu decode: no software H.264 decoder MFT ({e:?})");
            return;
        }

        let mut group = c.benchmark_group("decode");
        group.bench_function("sw_wmf_h264_cpu", |b| {
            b.iter_batched(
                || WindowsVideoDecoder::open(&cfg).expect("decoder open"),
                |mut dec| {
                    for packet in &packets {
                        dec.push_packet(packet).expect("push_packet");
                        while let Some(f) = dec.poll_frame().expect("poll_frame") {
                            black_box(f);
                        }
                    }
                    dec.flush().expect("flush");
                    while let Some(f) = dec.poll_frame().expect("poll_frame") {
                        black_box(f);
                    }
                },
                BatchSize::SmallInput,
            );
        });
        group.finish();
    }

    fn bench_zc_wmf_h264_dx11(c: &mut Criterion) {
        let Some((packets, extra_data)) = encode_h264_frames() else {
            eprintln!("skip zc_wmf_h264_dx11 decode: CPU H.264 encode setup failed");
            return;
        };

        // Try every present adapter, not just whichever `D3D_DRIVER_TYPE_HARDWARE` picks
        // by default — see `mediaway-encoder-windows/benches/wmf_h264_encode.rs` for the
        // same reasoning: not every GPU that is real hardware registers a D3D11-aware HW
        // H.264 decoder MFT with Media Foundation.
        //
        // `_device` must stay alive for the rest of this function: `device_handle` is a
        // borrowed raw pointer into it (`WindowsVideoDecoder::open` `AddRef`s its own
        // clone per call, but the original COM object must not be released first).
        let Some((_device, device_handle, adapter_name)) = open_decoder_on_any_adapter(&extra_data)
        else {
            eprintln!(
                "skip zc_wmf_h264_dx11 decode: no adapter on this machine exposes a HW H.264 decoder MFT"
            );
            return;
        };
        eprintln!("zc_wmf_h264_dx11 decode: HW adapter = {adapter_name}");

        let cfg = VideoDecoderConfig {
            codec: CodecKind::H264,
            width: WIDTH,
            height: HEIGHT,
            time_base: Rational::new(1, 30),
            pixel_format: PixelFormat::Nv12,
            output: VideoOutputPreference::ZeroCopyGpu,
            gpu_device: Some(GpuDeviceHandle::DirectX11(device_handle)),
            extra_data,
        };

        let mut group = c.benchmark_group("decode");
        group.bench_function("zc_wmf_h264_dx11", |b| {
            b.iter_batched(
                || WindowsVideoDecoder::open(&cfg).expect("decoder open"),
                |mut dec| {
                    for packet in &packets {
                        dec.push_packet(packet).expect("push_packet");
                        while let Some(f) = dec.poll_frame().expect("poll_frame") {
                            black_box(f);
                        }
                    }
                    dec.flush().expect("flush");
                    while let Some(f) = dec.poll_frame().expect("poll_frame") {
                        black_box(f);
                    }
                },
                BatchSize::SmallInput,
            );
        });
        group.finish();
    }

    /// Enumerate DXGI adapters and open a Zero-Copy H.264 decoder session on the first
    /// one that actually has a working D3D11-aware hardware decoder MFT. Skips the
    /// WARP/basic-render software adapter. Returns the owning `ID3D11Device` (caller must
    /// keep it alive — see call site), a handle to it (reused for every `iter_batched`
    /// decoder open below — `WindowsVideoDecoder::open` `AddRef`s its own clone each
    /// time), and a friendly adapter name for the report.
    fn open_decoder_on_any_adapter(
        extra_data: &Bytes,
    ) -> Option<(
        windows::Win32::Graphics::Direct3D11::ID3D11Device,
        NativeHandle,
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
                eprintln!("zc_wmf_h264_dx11 decode: D3D11CreateDevice on {name} failed ({hr:?})");
                continue;
            };
            let Some(device_handle) = NativeHandle::new(Interface::as_raw(&device) as usize) else {
                continue;
            };
            let cfg = VideoDecoderConfig {
                codec: CodecKind::H264,
                width: WIDTH,
                height: HEIGHT,
                time_base: Rational::new(1, 30),
                pixel_format: PixelFormat::Nv12,
                output: VideoOutputPreference::ZeroCopyGpu,
                gpu_device: Some(GpuDeviceHandle::DirectX11(device_handle)),
                extra_data: extra_data.clone(), // clone: probe open only, real config built by the caller
            };
            match WindowsVideoDecoder::open(&cfg) {
                Ok(_dec) => return Some((device, device_handle, name)),
                Err(e) => {
                    eprintln!(
                        "zc_wmf_h264_dx11 decode: {name} has no working HW H.264 decoder MFT ({e:?})"
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
