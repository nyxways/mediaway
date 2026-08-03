//! Windows encode backend (Media Foundation + DX11 Zero-Copy).
//!
//! - [`VideoInputPreference::CpuUploadOk`](crate::VideoInputPreference): sync inbox
//!   H.264 MFT + `upload_cpu_nv12` (copy); HEVC/AV1/VP9 via enumerated MFTs when present.
//! - [`VideoInputPreference::ZeroCopyGpu`](crate::VideoInputPreference): hardware MFT +
//!   `MFCreateDXGISurfaceBuffer` (requires `gpu_device`) for H.264 / HEVC / AV1 / VP9.
//! - [`auto`] — high-level path selection ([`auto::AutoVideoEncoder::open`]).
//!
//! Policy: [ADR-0001](../adr/0001-wmf-h264-surface.md), [ADR-0002](../adr/0002-windows-crate.md),
//! [ADR-0003](../adr/0003-dx11-zero-copy.md), [ADR-0004](../adr/0004-multi-codec-wmf.md),
//! [ADR-0005](../adr/0005-bgra-dxgi-input.md), [ADR-0006](../adr/0006-d3d12-shared-to-d3d11.md).
//!
//! Interop: [`D3d12SharedEncodeBridge`] — D3D12 shared heap → native D3D11 (`GpuCopy`).

#![cfg_attr(windows, allow(unsafe_code))]
#![cfg_attr(not(windows), deny(unsafe_code))]

#[cfg(all(not(feature = "audio"), not(feature = "video")))]
compile_error!("enable at least one of `audio` or `video` on mediaway-encoder-windows");

use crate::EncodeError;
#[cfg(feature = "audio")]
use crate::{AudioEncoder, AudioEncoderConfig};
#[cfg(feature = "video")]
use crate::{VideoEncoder, VideoEncoderConfig};
#[cfg(feature = "audio")]
use mediaway_common::AudioFrame;
#[cfg(feature = "video")]
use mediaway_common::VideoFrame;
use mediaway_common::{Bytes, Packet, StreamInfo};

#[cfg(feature = "video")]
pub mod auto;

#[cfg(all(windows, feature = "video"))]
mod d3d12_share;
#[cfg(all(windows, feature = "video"))]
pub use d3d12_share::D3d12SharedEncodeBridge;

// Not wired into `WindowsVideoEncoder`/`auto` yet — see the module's own doc comment.
// Declared (non-`pub`) so its `#[cfg(test)]` hardware-gated tests compile and run.
#[cfg(all(windows, feature = "video"))]
mod d3d12_video_encode;

#[cfg(all(windows, any(feature = "audio", feature = "video")))]
mod wmf;

/// Windows video encode session (H.264 MFT when opened on Windows).
#[cfg(feature = "video")]
pub struct WindowsVideoEncoder {
    #[cfg(windows)]
    inner: Option<wmf::WmfVideoEncoder>,
    #[cfg(not(windows))]
    _priv: (),
}

#[cfg(feature = "video")]
impl WindowsVideoEncoder {
    /// Open a Windows video encoder for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::Unsupported`] when the codec/path is not wired, or
    /// [`EncodeError::Backend`] on MF failure.
    #[cfg(windows)]
    pub fn open(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        let inner = wmf::WmfVideoEncoder::open(config)?;
        Ok(Self { inner: Some(inner) })
    }

    /// Host / non-Windows build: encoder unavailable.
    #[cfg(not(windows))]
    pub const fn open(_config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        Err(EncodeError::Unsupported)
    }
}

#[cfg(feature = "video")]
#[cfg(windows)]
impl VideoEncoder for WindowsVideoEncoder {
    fn stream_info(&self) -> &StreamInfo {
        #[allow(
            clippy::option_if_let_else,
            reason = "map_or_else forces 'static vs 'self lifetime clash"
        )]
        if let Some(e) = self.inner.as_ref() {
            e.stream_info()
        } else {
            closed_stream_info()
        }
    }

    fn push_frame(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        self.inner
            .as_mut()
            .ok_or(EncodeError::Closed)?
            .push_frame(frame)
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        self.inner
            .as_mut()
            .ok_or(EncodeError::Closed)?
            .poll_packet()
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        self.inner.as_mut().ok_or(EncodeError::Closed)?.flush()
    }
}

#[cfg(feature = "video")]
#[cfg(not(windows))]
impl VideoEncoder for WindowsVideoEncoder {
    fn stream_info(&self) -> &StreamInfo {
        closed_stream_info()
    }

    fn push_frame(&mut self, _frame: &VideoFrame) -> Result<(), EncodeError> {
        Err(EncodeError::Unsupported)
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        Ok(None)
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        Err(EncodeError::Unsupported)
    }
}

#[cfg(feature = "video")]
fn closed_stream_info() -> &'static StreamInfo {
    use std::sync::OnceLock;
    static INFO: OnceLock<StreamInfo> = OnceLock::new();
    INFO.get_or_init(|| StreamInfo::Video {
        id: 0,
        codec: mediaway_common::CodecKind::H264,
        time_base: mediaway_common::Rational::new(1, 30),
        geometry: mediaway_common::VideoGeometry {
            width: 0,
            height: 0,
        },
        extra_data: Bytes::new(),
    })
}

/// Windows audio encode session (WMF AAC when opened on Windows).
#[cfg(feature = "audio")]
pub struct WindowsAudioEncoder {
    #[cfg(windows)]
    inner: Option<wmf::WmfAacEncoder>,
    #[cfg(not(windows))]
    _priv: (),
}

#[cfg(feature = "audio")]
impl WindowsAudioEncoder {
    /// Open a Windows audio encoder for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::Unsupported`] when the codec/path is not wired, or
    /// [`EncodeError::Backend`] on MF failure.
    #[cfg(windows)]
    pub fn open(config: &AudioEncoderConfig) -> Result<Self, EncodeError> {
        let inner = wmf::WmfAacEncoder::open(config)?;
        Ok(Self { inner: Some(inner) })
    }

    /// Host / non-Windows build: encoder unavailable.
    #[cfg(not(windows))]
    pub const fn open(_config: &AudioEncoderConfig) -> Result<Self, EncodeError> {
        Err(EncodeError::Unsupported)
    }

    /// Build a closed placeholder (tests / future MF open path).
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "StreamInfo holds Bytes; MF open will not be const"
    )]
    pub fn placeholder(_config: &AudioEncoderConfig) -> Self {
        #[cfg(windows)]
        {
            Self { inner: None }
        }
        #[cfg(not(windows))]
        {
            Self { _priv: () }
        }
    }
}

#[cfg(feature = "audio")]
#[cfg(windows)]
impl AudioEncoder for WindowsAudioEncoder {
    fn stream_info(&self) -> &StreamInfo {
        #[allow(
            clippy::option_if_let_else,
            reason = "map_or_else forces 'static vs 'self lifetime clash"
        )]
        if let Some(e) = self.inner.as_ref() {
            e.stream_info()
        } else {
            closed_audio_stream_info()
        }
    }

    fn push_frame(&mut self, frame: &AudioFrame) -> Result<(), EncodeError> {
        self.inner
            .as_mut()
            .ok_or(EncodeError::Closed)?
            .push_frame(frame)
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        self.inner
            .as_mut()
            .ok_or(EncodeError::Closed)?
            .poll_packet()
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        self.inner.as_mut().ok_or(EncodeError::Closed)?.flush()
    }
}

#[cfg(feature = "audio")]
#[cfg(not(windows))]
impl AudioEncoder for WindowsAudioEncoder {
    fn stream_info(&self) -> &StreamInfo {
        closed_audio_stream_info()
    }

    fn push_frame(&mut self, _frame: &AudioFrame) -> Result<(), EncodeError> {
        Err(EncodeError::Unsupported)
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        Ok(None)
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        Err(EncodeError::Unsupported)
    }
}

#[cfg(feature = "audio")]
fn closed_audio_stream_info() -> &'static StreamInfo {
    use std::sync::OnceLock;
    static INFO: OnceLock<StreamInfo> = OnceLock::new();
    INFO.get_or_init(|| StreamInfo::Audio {
        id: 0,
        codec: mediaway_common::CodecKind::Aac,
        time_base: mediaway_common::Rational::new(1, 48_000),
        extra_data: Bytes::new(),
        sample_rate: 0,
        channels: 0,
    })
}

#[cfg(all(test, windows, feature = "video"))]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]
mod tests {
    use super::*;
    use crate::VideoInputPreference;
    use mediaway_common::{
        CodecKind, GpuDeviceHandle, NativeHandle, PixelFormat, Rational, VideoFrameStorage,
    };

    #[test]
    fn open_h264_encodes_black_nv12_frame() {
        let cfg = VideoEncoderConfig {
            codec: CodecKind::H264,
            width: 64,
            height: 64,
            time_base: Rational::new(1, 30),
            bitrate_bps: 500_000,
            pixel_format: PixelFormat::Nv12,
            input: VideoInputPreference::CpuUploadOk,
            gpu_device: None,
        };
        let mut enc = match WindowsVideoEncoder::open(&cfg) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("skip: WindowsVideoEncoder::open failed ({e:?}) — MF unavailable?");
                return;
            }
        };
        let nv12_len = 64 * 64 + 64 * 64 / 2;
        let frame = VideoFrame {
            pts: 0,
            duration: 1,
            width: 64,
            height: 64,
            format: PixelFormat::Nv12,
            storage: VideoFrameStorage::Cpu {
                data: Bytes::from(vec![0u8; nv12_len]),
            },
        };
        enc.push_frame(&frame).expect("push");
        enc.flush().expect("flush");
        let mut packets = 0usize;
        while let Some(p) = enc.poll_packet().expect("poll") {
            assert!(!p.payload.is_empty());
            packets += 1;
        }
        assert!(packets >= 1, "expected at least one encoded packet");
    }

    #[test]
    fn open_dx11_zero_copy_or_skip_without_hw() {
        use windows::Win32::Foundation::HMODULE;
        use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
        use windows::Win32::Graphics::Direct3D11::{
            D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION,
            D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11CreateDevice, ID3D11Device,
            ID3D11Texture2D,
        };
        use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
        use windows::core::Interface;

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
        let cfg = VideoEncoderConfig {
            codec: CodecKind::H264,
            width: 64,
            height: 64,
            time_base: Rational::new(1, 30),
            bitrate_bps: 500_000,
            pixel_format: PixelFormat::Nv12,
            input: VideoInputPreference::ZeroCopyGpu,
            gpu_device: Some(GpuDeviceHandle::DirectX11(device_handle)),
        };
        let mut enc = match WindowsVideoEncoder::open(&cfg) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("skip: no HW H.264 MFT / DXGI path ({e:?})");
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
        if unsafe { device.CreateTexture2D(&raw const desc, None, Some(&raw mut texture)) }.is_err()
        {
            eprintln!("skip: CreateTexture2D NV12 failed");
            return;
        }
        let texture = texture.expect("texture");
        let frame = VideoFrame {
            pts: 0,
            duration: 1,
            width: 64,
            height: 64,
            format: PixelFormat::Nv12,
            storage: VideoFrameStorage::Gpu(mediaway_common::GpuBufferHandle::DirectX11 {
                texture: NativeHandle::new(Interface::as_raw(&texture) as usize)
                    .expect("texture pointer"),
                subresource: 0,
            }),
        };
        enc.push_frame(&frame).expect("dx11 push");
        enc.flush().expect("flush");
        let mut packets = 0usize;
        while let Some(p) = enc.poll_packet().expect("poll") {
            assert!(!p.payload.is_empty());
            packets += 1;
        }
        eprintln!("dx11 packets={packets}");
    }

    #[test]
    fn open_hevc_av1_vp9_cpu_or_skip() {
        for codec in [CodecKind::Hevc, CodecKind::Av1, CodecKind::Vp9] {
            let cfg = VideoEncoderConfig {
                codec,
                width: 64,
                height: 64,
                time_base: Rational::new(1, 30),
                bitrate_bps: 500_000,
                pixel_format: PixelFormat::Nv12,
                input: VideoInputPreference::CpuUploadOk,
                gpu_device: None,
            };
            let mut enc = match WindowsVideoEncoder::open(&cfg) {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("skip: {codec:?} CPU encode open ({e:?})");
                    continue;
                }
            };
            let nv12_len = 64 * 64 + 64 * 64 / 2;
            let frame = VideoFrame {
                pts: 0,
                duration: 1,
                width: 64,
                height: 64,
                format: PixelFormat::Nv12,
                storage: VideoFrameStorage::Cpu {
                    data: Bytes::from(vec![0u8; nv12_len]),
                },
            };
            if let Err(e) = enc.push_frame(&frame) {
                eprintln!("skip: {codec:?} push ({e:?})");
                continue;
            }
            let _ = enc.flush();
            let mut packets = 0usize;
            while let Ok(Some(p)) = enc.poll_packet() {
                assert!(!p.payload.is_empty());
                packets += 1;
            }
            eprintln!("{codec:?} cpu packets={packets}");
        }
    }

    #[test]
    fn open_hevc_av1_vp9_dx11_or_skip() {
        use windows::Win32::Foundation::HMODULE;
        use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
        use windows::Win32::Graphics::Direct3D11::{
            D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION,
            D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11CreateDevice, ID3D11Device,
            ID3D11Texture2D,
        };
        use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
        use windows::core::Interface;

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
        if unsafe { device.CreateTexture2D(&raw const desc, None, Some(&raw mut texture)) }.is_err()
        {
            eprintln!("skip: CreateTexture2D NV12 failed");
            return;
        }
        let texture = texture.expect("texture");
        let tex_handle =
            NativeHandle::new(Interface::as_raw(&texture) as usize).expect("texture pointer");

        for codec in [CodecKind::Hevc, CodecKind::Av1, CodecKind::Vp9] {
            let cfg = VideoEncoderConfig {
                codec,
                width: 64,
                height: 64,
                time_base: Rational::new(1, 30),
                bitrate_bps: 500_000,
                pixel_format: PixelFormat::Nv12,
                input: VideoInputPreference::ZeroCopyGpu,
                gpu_device: Some(GpuDeviceHandle::DirectX11(device_handle)),
            };
            let mut enc = match WindowsVideoEncoder::open(&cfg) {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("skip: {codec:?} DX11 encode open ({e:?})");
                    continue;
                }
            };
            let frame = VideoFrame {
                pts: 0,
                duration: 1,
                width: 64,
                height: 64,
                format: PixelFormat::Nv12,
                storage: VideoFrameStorage::Gpu(mediaway_common::GpuBufferHandle::DirectX11 {
                    texture: tex_handle,
                    subresource: 0,
                }),
            };
            if let Err(e) = enc.push_frame(&frame) {
                eprintln!("skip: {codec:?} dx11 push ({e:?})");
                continue;
            }
            let _ = enc.flush();
            eprintln!("{codec:?} dx11 open+push ok");
        }
    }

    #[test]
    fn d3d12_shared_bridge_open_or_skip() {
        use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
        use windows::Win32::Graphics::Direct3D12::{D3D12CreateDevice, ID3D12Device};
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1};
        use windows::core::Interface;

        let factory: IDXGIFactory1 = match unsafe { CreateDXGIFactory1() } {
            Ok(f) => f,
            Err(e) => {
                eprintln!("skip: CreateDXGIFactory1 ({e:?})");
                return;
            }
        };
        let adapter: IDXGIAdapter1 = match unsafe { factory.EnumAdapters1(0) } {
            Ok(a) => a,
            Err(e) => {
                eprintln!("skip: EnumAdapters1 ({e:?})");
                return;
            }
        };
        let mut device: Option<ID3D12Device> = None;
        if unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &raw mut device) }.is_err()
        {
            eprintln!("skip: D3D12CreateDevice failed");
            return;
        }
        let Some(device) = device else {
            eprintln!("skip: null D3D12 device");
            return;
        };
        let Some(handle) = NativeHandle::new(Interface::as_raw(&device) as usize) else {
            eprintln!("skip: null D3D12 device pointer");
            return;
        };
        match D3d12SharedEncodeBridge::open(handle, 64, 64) {
            Ok(bridge) => {
                assert!(bridge.d3d11_texture_handle().is_ok());
                assert!(bridge.d3d12_resource_handle().is_ok());
                eprintln!("d3d12 shared bridge ok");
            }
            Err(e) => eprintln!("skip: D3d12SharedEncodeBridge::open ({e:?})"),
        }
    }

    /// End-to-end `GpuCopy`: a real `ID3D12Device` bridged to native D3D11 via
    /// [`D3d12SharedEncodeBridge`], then opened through
    /// [`auto::AutoVideoEncoder::open`]. Skip gracefully on any missing
    /// D3D12/D3D11/HW-MFT capability rather than failing the default suite on
    /// machines without that hardware.
    #[test]
    fn auto_open_gpu_copy_via_d3d12_bridge_or_skip() {
        use crate::auto::{AutoVideoEncodeConfig, EncodePathClass};
        use crate::windows::auto::AutoVideoEncoder;
        use mediaway_common::GpuBufferHandle;
        use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
        use windows::Win32::Graphics::Direct3D12::{D3D12CreateDevice, ID3D12Device};
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1};
        use windows::core::Interface;

        let factory: IDXGIFactory1 = match unsafe { CreateDXGIFactory1() } {
            Ok(f) => f,
            Err(e) => {
                eprintln!("skip: CreateDXGIFactory1 ({e:?})");
                return;
            }
        };
        let adapter: IDXGIAdapter1 = match unsafe { factory.EnumAdapters1(0) } {
            Ok(a) => a,
            Err(e) => {
                eprintln!("skip: EnumAdapters1 ({e:?})");
                return;
            }
        };
        let mut device: Option<ID3D12Device> = None;
        if unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &raw mut device) }.is_err()
        {
            eprintln!("skip: D3D12CreateDevice failed");
            return;
        }
        let Some(device) = device else {
            eprintln!("skip: null D3D12 device");
            return;
        };
        let Some(handle) = NativeHandle::new(Interface::as_raw(&device) as usize) else {
            eprintln!("skip: null D3D12 device pointer");
            return;
        };

        let cfg = AutoVideoEncodeConfig {
            gpu_device: Some(GpuDeviceHandle::DirectX12(handle)),
            ..AutoVideoEncodeConfig::new(CodecKind::H264, 64, 64, Rational::new(1, 30))
        };

        let mut enc = match AutoVideoEncoder::open(&cfg) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("skip: AutoVideoEncoder::open GpuCopy ({e:?})");
                return;
            }
        };
        if enc.path_class() != EncodePathClass::GpuCopy {
            // `try_gpu_copy` bridges the D3D12 adapter chosen by `EnumAdapters1(0)`
            // to native D3D11, then still needs a HW encoder MFT on *that specific*
            // adapter. On multi-adapter machines that can differ from the adapter
            // `D3D_DRIVER_TYPE_HARDWARE` (used elsewhere in this suite) would pick,
            // so `open` honestly falls back to CPU upload instead — not a failure,
            // just this adapter/environment lacking the capability.
            eprintln!(
                "skip: GpuCopy unavailable on this adapter, fell back to {:?}",
                enc.path_class()
            );
            return;
        }
        let Some(_copy_target) = enc.gpu_copy_target() else {
            eprintln!("skip: no gpu_copy_target after GpuCopy open");
            return;
        };
        let Some(GpuBufferHandle::DirectX11 { texture, .. }) = enc.gpu_copy_dx11_frame_handle()
        else {
            eprintln!("skip: no gpu_copy_dx11_frame_handle after GpuCopy open");
            return;
        };
        // caller would `CopyResource` into `_copy_target` (the D3D12 resource)
        // once per frame before pushing the DX11 view below.
        let frame = VideoFrame {
            pts: 0,
            duration: 1,
            width: 64,
            height: 64,
            format: PixelFormat::Nv12,
            storage: VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 {
                texture,
                subresource: 0,
            }),
        };
        if let Err(e) = enc.push_frame(&frame) {
            eprintln!("skip: gpu copy push ({e:?})");
            return;
        }
        let _ = enc.flush();
        let mut packets = 0usize;
        while let Ok(Some(p)) = enc.poll_packet() {
            assert!(!p.payload.is_empty());
            packets += 1;
        }
        eprintln!("gpu copy packets={packets}");
    }
}

#[cfg(all(test, windows, feature = "audio"))]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]
mod audio_tests {
    use super::*;
    use crate::AudioEncoderConfig;
    use mediaway_common::{CodecKind, Rational};

    #[test]
    fn open_aac_encodes_silence_pcm() {
        let cfg = AudioEncoderConfig {
            codec: CodecKind::Aac,
            sample_rate: 48_000,
            channels: 2,
            sample_format: mediaway_common::SampleFormat::S16,
            time_base: Rational::new(1, 48_000),
            bitrate_bps: 128_000,
        };
        let mut enc = match WindowsAudioEncoder::open(&cfg) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("skip: WindowsAudioEncoder::open failed ({e:?}) — MF unavailable?");
                return;
            }
        };
        let samples = 2048usize;
        let bytes = samples * 2 * 2;
        let frame = AudioFrame {
            pts: 0,
            duration: samples as u64,
            sample_rate: 48_000,
            channels: 2,
            format: mediaway_common::SampleFormat::S16,
            data: Bytes::from(vec![0u8; bytes]),
        };
        enc.push_frame(&frame).expect("push");
        enc.flush().expect("flush");
        let mut packets = 0usize;
        while let Some(p) = enc.poll_packet().expect("poll") {
            assert!(!p.payload.is_empty());
            packets += 1;
        }
        assert!(packets >= 1, "expected at least one AAC packet");
    }
}
