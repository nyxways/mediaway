//! Windows decode backend (Media Foundation + DX11 Zero-Copy output).
//!
//! - [`VideoOutputPreference::ZeroCopyGpu`](crate::VideoOutputPreference): hardware
//!   H.264 decoder MFT + DXGI output surfaces (requires `gpu_device`).
//! - [`VideoOutputPreference::CpuFramesOk`](crate::VideoOutputPreference): software
//!   H.264 decoder MFT; frames copied straight from its system-memory output buffer (no GPU
//!   involved, so this is honest CPU decode, not a GPU→CPU readback). Also HEVC/AV1/VP9 CPU-only
//!   decode via enumerated MFT decoder (no DX11 Zero-Copy path for these codecs).
//!
//! Policy: [ADR-0001](../adr/0001-wmf-h264-dx11-out.md).
//!
//! Interop: [`D3d11SharedDecodeBridge`] — WMF DX11 Zero-Copy decode output → shared D3D12
//! resource for `mediaway-wgpu` (`GpuCopy`, [ADR-0003](../adr/0003-d3d11-shared-decode-bridge.md)).

#![cfg_attr(windows, allow(unsafe_code))]
#![cfg_attr(not(windows), deny(unsafe_code))]

#[cfg(not(feature = "video"))]
compile_error!("enable the `video` feature on mediaway-decoder-windows");

use crate::DecodeError;
#[cfg(feature = "video")]
use crate::{VideoDecoder, VideoDecoderConfig};
#[cfg(feature = "video")]
use mediaway_common::VideoFrame;
use mediaway_common::{Bytes, Packet, StreamInfo};

#[cfg(all(windows, feature = "video"))]
mod d3d11_shared_decode_bridge;
#[cfg(all(windows, feature = "video"))]
pub use d3d11_shared_decode_bridge::D3d11SharedDecodeBridge;

/// Public WMF Opus decode session (inbox `CMSOpusDecMFT`, Float32 PCM out).
/// Reachable as `mediaway_decoder::windows::WmfOpusDecoder` — real, tested
/// MFT plumbing (see `wmf::opus` module docs); no facade `AudioDecoder`
/// trait exists yet, so this is the low-level first-class entry.
#[cfg(all(windows, feature = "audio"))]
pub use wmf::opus::{OpusDecoderConfig, WmfOpusDecoder};

#[cfg(all(windows, any(feature = "video", feature = "audio")))]
mod wmf;

// Not wired into `WindowsVideoDecoder` yet — see the module's own doc comment
// (ADR-0002). Declared (non-`pub`) so its `#[cfg(test)]` hardware-gated tests
// compile and run, same trick `mediaway-encoder-windows` uses for
// `d3d12_video_encode`.
#[cfg(all(windows, feature = "video"))]
mod d3d12_video_decode;

#[cfg(windows)]
enum Backend {
    H264(wmf::WmfH264Decoder),
    MultiCodecCpu(wmf::WmfMultiCodecCpuDecoder),
}

/// Windows video decode session (H.264 or HEVC/AV1/VP9 MFT when opened on Windows).
#[cfg(feature = "video")]
pub struct WindowsVideoDecoder {
    #[cfg(windows)]
    inner: Option<Backend>,
    #[cfg(not(windows))]
    _priv: (),
}

#[cfg(feature = "video")]
impl WindowsVideoDecoder {
    /// Open a Windows video decoder for `config`.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::Unsupported`] when the codec/path is not wired, or
    /// [`DecodeError::Backend`] on MF failure.
    #[cfg(windows)]
    pub fn open(config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        use mediaway_common::CodecKind;
        let inner = match config.codec {
            CodecKind::H264 => Backend::H264(wmf::WmfH264Decoder::open(config)?),
            CodecKind::Hevc | CodecKind::Av1 | CodecKind::Vp9 => {
                Backend::MultiCodecCpu(wmf::WmfMultiCodecCpuDecoder::open(config)?)
            }
            _ => return Err(DecodeError::Unsupported),
        };
        Ok(Self { inner: Some(inner) })
    }

    /// Host / non-Windows build: decoder unavailable.
    #[cfg(not(windows))]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "DecodeError path; non-const when MF open lands on Windows"
    )]
    pub fn open(_config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        Err(DecodeError::Unsupported)
    }
}

#[cfg(feature = "video")]
#[cfg(windows)]
impl VideoDecoder for WindowsVideoDecoder {
    fn stream_info(&self) -> &StreamInfo {
        #[allow(
            clippy::option_if_let_else,
            reason = "map_or_else forces 'static vs 'self lifetime clash"
        )]
        if let Some(backend) = self.inner.as_ref() {
            match backend {
                Backend::H264(d) => d.stream_info(),
                Backend::MultiCodecCpu(d) => d.stream_info(),
            }
        } else {
            closed_stream_info()
        }
    }

    fn push_packet(&mut self, packet: &Packet) -> Result<(), DecodeError> {
        match self.inner.as_mut().ok_or(DecodeError::Closed)? {
            Backend::H264(d) => d.push_packet(packet),
            Backend::MultiCodecCpu(d) => d.push_packet(packet),
        }
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        match self.inner.as_mut().ok_or(DecodeError::Closed)? {
            Backend::H264(d) => d.poll_frame(),
            Backend::MultiCodecCpu(d) => d.poll_frame(),
        }
    }

    fn flush(&mut self) -> Result<(), DecodeError> {
        match self.inner.as_mut().ok_or(DecodeError::Closed)? {
            Backend::H264(d) => d.flush(),
            Backend::MultiCodecCpu(d) => d.flush(),
        }
    }
}

#[cfg(feature = "video")]
#[cfg(not(windows))]
impl VideoDecoder for WindowsVideoDecoder {
    fn stream_info(&self) -> &StreamInfo {
        closed_stream_info()
    }

    fn push_packet(&mut self, _packet: &Packet) -> Result<(), DecodeError> {
        Err(DecodeError::Unsupported)
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        Ok(None)
    }

    fn flush(&mut self) -> Result<(), DecodeError> {
        Err(DecodeError::Unsupported)
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

#[cfg(all(test, windows, feature = "video"))]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]
mod tests {
    use super::*;
    use crate::VideoOutputPreference;
    use mediaway_common::{CodecKind, GpuDeviceHandle, NativeHandle, PixelFormat, Rational};

    #[test]
    fn open_dx11_zero_copy_or_skip() {
        use windows::Win32::Foundation::HMODULE;
        use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
        use windows::Win32::Graphics::Direct3D11::{
            D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
        };
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
        let cfg = VideoDecoderConfig {
            codec: CodecKind::H264,
            width: 64,
            height: 64,
            time_base: Rational::new(1, 30),
            pixel_format: PixelFormat::Nv12,
            output: VideoOutputPreference::ZeroCopyGpu,
            gpu_device: Some(GpuDeviceHandle::DirectX11(device_handle)),
            extra_data: Bytes::new(),
        };
        let mut dec = match WindowsVideoDecoder::open(&cfg) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("skip: no HW H.264 decoder MFT / DXGI path ({e:?})");
                return;
            }
        };
        dec.flush().expect("flush without packets");
        assert!(dec.poll_frame().expect("poll").is_none());
    }
}
