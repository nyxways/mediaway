#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use windows::Win32::Media::MediaFoundation::{
    IMFActivate, MFMediaType_Video, MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG,
    MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER, MFT_FRIENDLY_NAME_Attribute,
    MFT_REGISTER_TYPE_INFO, MFTEnumEx, MFVideoFormat_AV1, MFVideoFormat_HEVC, MFVideoFormat_VP90,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::core::PWSTR;

/// Real `MFTEnumEx(MFT_CATEGORY_VIDEO_ENCODER, …)` results for HEVC / AV1 / VP9 on this
/// machine, both unfiltered (`MFT_ENUM_FLAG_SORTANDFILTER` only — any encoder MFT, HW or SW,
/// that declares the subtype, mirroring `activate_encoder_mft(_, false)`'s CPU-open call
/// shape) and `MFT_ENUM_FLAG_HARDWARE`-filtered (mirroring `activate_encoder_mft(_,
/// true)`'s DX11-open call shape). Informational: records real findings either way rather
/// than asserting a specific outcome, since which encoder MFTs are registered is a property
/// of the OS/driver install, not this crate — mirrors
/// `mediaway-decoder-windows`'s own `list_decoder_mfts_for_each_codec` doc-comment stance.
/// See `docs/roadmap.md` for the findings this produced on the verification host.
#[test]
fn list_encoder_mfts_for_each_codec() {
    super::super::runtime::ensure_mf().expect("MF runtime init");
    for (name, subtype) in [
        ("HEVC", MFVideoFormat_HEVC),
        ("AV1", MFVideoFormat_AV1),
        ("VP9", MFVideoFormat_VP90),
    ] {
        let unfiltered = enum_encoder_mft_names(subtype, false);
        let hw_only = enum_encoder_mft_names(subtype, true);
        eprintln!("{name}: any-flag encoder MFTs = {unfiltered:?}");
        eprintln!("{name}: MFT_ENUM_FLAG_HARDWARE encoder MFTs = {hw_only:?}");
    }
}

/// Real `MFTEnumEx` call + friendly-name lookup for every registered
/// `MFT_CATEGORY_VIDEO_ENCODER` MFT that declares `subtype` as an accepted output.
/// `hardware_only` mirrors `activate_encoder_mft`'s own two flag/input-filter shapes: the
/// hardware path passes no input-type filter (live-recorder pattern, DX11 Zero-Copy open),
/// the non-hardware path filters on NV12 input (CPU-upload open).
fn enum_encoder_mft_names(subtype: windows::core::GUID, hardware_only: bool) -> Vec<String> {
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: subtype,
    };
    let flags = if hardware_only {
        MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_SORTANDFILTER.0)
    } else {
        MFT_ENUM_FLAG_SORTANDFILTER
    };
    let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    // SAFETY: MFTEnumEx writes an activate-object array + count as out-params; freed below.
    let hr = unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            flags,
            None,
            Some(std::ptr::from_ref(&output)),
            &raw mut activates,
            &raw mut count,
        )
    };
    if hr.is_err() || activates.is_null() {
        return Vec::new();
    }
    let mut names = Vec::new();
    for i in 0..count as usize {
        // SAFETY: `activates` holds `count` valid `Option<IMFActivate>` slots from MFTEnumEx.
        let activate = unsafe { (*activates.add(i)).take() };
        if let Some(activate) = activate {
            names.push(friendly_name(&activate).unwrap_or_else(|| "<unnamed>".to_owned()));
        }
    }
    // SAFETY: `activates` was allocated by MFTEnumEx (CoTaskMemAlloc); we own and free it.
    unsafe {
        CoTaskMemFree(Some(activates.cast_const().cast()));
    }
    names
}

fn friendly_name(activate: &IMFActivate) -> Option<String> {
    let mut raw = PWSTR::null();
    let mut len = 0u32;
    // SAFETY: out-params written on success; the string is `CoTaskMemAlloc`'d and freed below.
    unsafe {
        activate.GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &raw mut raw, &raw mut len)
    }
    .ok()?;
    if raw.is_null() {
        return None;
    }
    // SAFETY: `raw` is a valid null-terminated wide string per `GetAllocatedString`'s
    // contract, still valid at this point (freed only below).
    let name = unsafe { raw.to_string() }.ok();
    // SAFETY: matching `CoTaskMemFree` for the successful `GetAllocatedString` above.
    unsafe {
        CoTaskMemFree(Some(raw.0.cast()));
    }
    name
}

#[cfg(test)]
mod zero_copy {
    use crate::{VideoEncoder as _, VideoEncoderConfig, VideoInputPreference};
    use mediaway_common::{
        CodecKind, ColorRange, GpuBufferHandle, GpuDeviceHandle, NativeHandle, PixelFormat,
        Rational, VideoFrame, VideoFrameStorage,
    };
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
        D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11CreateDevice,
        ID3D11Device, ID3D11Texture2D,
    };
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC,
    };
    use windows::core::Interface;

    const W: u32 = 1920;
    const H: u32 = 1080;
    const FRAMES: i64 = 5;

    fn hardware_device() -> Option<ID3D11Device> {
        let mut device: Option<ID3D11Device> = None;
        // SAFETY: standard D3D11 device creation; the out-param is checked by the caller.
        unsafe {
            let _ = D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&raw mut device),
                None,
                None,
            );
        }
        device
    }

    fn texture(device: &ID3D11Device, format: DXGI_FORMAT, bgra: bool) -> Option<ID3D11Texture2D> {
        let mut bind = D3D11_BIND_SHADER_RESOURCE.0 as u32;
        if bgra {
            bind |= D3D11_BIND_RENDER_TARGET.0 as u32;
        }
        let desc = D3D11_TEXTURE2D_DESC {
            Width: W,
            Height: H,
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: bind,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut out: Option<ID3D11Texture2D> = None;
        // SAFETY: `desc` is a fully initialized POD descriptor; out-param checked by caller.
        unsafe {
            let _ = device.CreateTexture2D(&raw const desc, None, Some(&raw mut out));
        }
        out
    }

    /// `Some(packets)` when the case ran, `None` when this machine cannot exercise it.
    fn encode_case(
        device: &ID3D11Device,
        codec: CodecKind,
        pixel_format: PixelFormat,
        format: DXGI_FORMAT,
    ) -> Option<usize> {
        // The texture is created **before** the encoder so it outlives it: locals drop in
        // reverse declaration order, and the encoder's MFT still holds samples that
        // reference this surface until it is dropped.
        let texture = texture(device, format, pixel_format == PixelFormat::Bgra8)?;
        let texture_handle = NativeHandle::new(Interface::as_raw(&texture) as usize)?;
        let device_handle = NativeHandle::new(Interface::as_raw(device) as usize)?;

        let config = VideoEncoderConfig {
            codec,
            width: W,
            height: H,
            time_base: Rational::new(1, 60),
            bitrate_bps: 20_000_000,
            pixel_format,
            color_range: ColorRange::Video,
            input: VideoInputPreference::ZeroCopyGpu,
            gpu_device: Some(GpuDeviceHandle::DirectX11(device_handle)),
            gop_size: 120,
            rate_control: None,
            intra_refresh_period: None,
        };
        let mut encoder = super::super::WmfVideoEncoder::open(&config).ok()?;

        for pts in 0..FRAMES {
            let frame = VideoFrame {
                pts,
                duration: 1,
                width: W,
                height: H,
                format: pixel_format,
                storage: VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 {
                    texture: texture_handle,
                    subresource: 0,
                }),
            };
            encoder
                .push_frame(&frame)
                .expect("async MFT must accept every frame, not just the first");
        }
        encoder.flush().expect("flush");

        let mut packets = 0;
        while encoder.poll_packet().expect("poll").is_some() {
            packets += 1;
        }
        drop(encoder);
        drop(texture);
        Some(packets)
    }

    /// Regression for the three async-MFT sequencing bugs in
    /// `adr/windows/0012-async-mft-zero-copy-sequencing.md`. Each surfaced at a different
    /// stage — `open`, the first `push_frame`, and the *second* `push_frame` — so this
    /// pushes several frames rather than one, and asserts packets actually came out.
    ///
    /// Skips honestly without a hardware encoder MFT.
    #[test]
    fn dx11_zero_copy_encodes_multiple_frames_or_skip() {
        let Some(device) = hardware_device() else {
            eprintln!("skip: no D3D11 hardware device");
            return;
        };
        for (codec, pixel_format, format) in [
            (CodecKind::H264, PixelFormat::Nv12, DXGI_FORMAT_NV12),
            (
                CodecKind::H264,
                PixelFormat::Bgra8,
                DXGI_FORMAT_B8G8R8A8_UNORM,
            ),
            (CodecKind::Hevc, PixelFormat::Nv12, DXGI_FORMAT_NV12),
            (
                CodecKind::Hevc,
                PixelFormat::Bgra8,
                DXGI_FORMAT_B8G8R8A8_UNORM,
            ),
        ] {
            match encode_case(&device, codec, pixel_format, format) {
                Some(packets) => assert!(
                    packets > 0,
                    "{codec:?} {pixel_format:?}: encoded {FRAMES} frames, produced no packets",
                ),
                None => eprintln!("skip: {codec:?} {pixel_format:?} unavailable on this machine"),
            }
        }
    }

    /// Stress regression for mediaway#106. Releasing an async hardware MFT (NVIDIA's) right
    /// after use raced work it still had in flight, and the process died with an access
    /// violation on a Media Foundation work-queue thread. Dropping an encoder *without*
    /// flushing it is the sharpest trigger. Measured on an RTX 4090, it crashed on about 3% of
    /// drops, and 138 of 200 processes doing 40 drops each died. This test does 200 unflushed
    /// drops in a row: with the defect, a run survives that about 0.2% of the time
    /// (0.97^200). With the fix, 8 000 such drops produced no crash. A failure here is the
    /// process dying, not an assertion.
    ///
    /// `#[ignore]`d for time (200 × the 50 ms release grace ≈ 10 s plus encoding), not for
    /// touching the desktop:
    ///
    /// ```text
    /// cargo nextest run -p mediaway-encoder --run-ignored only -E 'test(dropping_encoders)'
    /// ```
    #[test]
    #[ignore = "slow stress test; run explicitly"]
    fn dropping_encoders_back_to_back_does_not_crash() {
        let Some(device) = hardware_device() else {
            eprintln!("skip: no D3D11 hardware device");
            return;
        };
        let Some(texture) = texture(&device, DXGI_FORMAT_NV12, false) else {
            return;
        };
        let Some(texture_handle) = NativeHandle::new(Interface::as_raw(&texture) as usize) else {
            return;
        };
        let Some(device_handle) = NativeHandle::new(Interface::as_raw(&device) as usize) else {
            return;
        };
        let config = VideoEncoderConfig {
            codec: CodecKind::H264,
            width: W,
            height: H,
            time_base: Rational::new(1, 60),
            bitrate_bps: 20_000_000,
            pixel_format: PixelFormat::Nv12,
            color_range: ColorRange::Video,
            input: VideoInputPreference::ZeroCopyGpu,
            gpu_device: Some(GpuDeviceHandle::DirectX11(device_handle)),
            gop_size: 120,
            rate_control: None,
            intra_refresh_period: None,
        };
        for i in 0..200 {
            let Ok(mut encoder) = super::super::WmfVideoEncoder::open(&config) else {
                eprintln!("skip: no hardware H.264 encoder (at iteration {i})");
                return;
            };
            for pts in 0..FRAMES {
                let frame = VideoFrame {
                    pts,
                    duration: 1,
                    width: W,
                    height: H,
                    format: PixelFormat::Nv12,
                    storage: VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 {
                        texture: texture_handle,
                        subresource: 0,
                    }),
                };
                encoder.push_frame(&frame).expect("push");
            }
            // Deliberately no flush: the path a recorder takes when it stops or a window
            // resizes, and the sharpest trigger for the race.
            drop(encoder);
        }
    }
}
