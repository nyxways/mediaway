//! Tests for Windows [`super::AutoVideoEncoder::open`].
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test modules may unwrap and report hardware-gated skips"
)]

use super::AutoVideoEncoder;
use crate::EncodeError;
use crate::VideoEncoder;
use crate::auto::{AutoVideoEncodeConfig, Backend, BackendSelection, EncodePathClass};
use mediaway_common::{
    CodecKind, GpuDeviceHandle, NativeHandle, PixelFormat, Rational, VideoFrame, VideoFrameStorage,
};

fn tiny_h264() -> AutoVideoEncodeConfig {
    AutoVideoEncodeConfig::new(CodecKind::H264, 64, 64, Rational::new(1, 30))
}

#[cfg(windows)]
#[cfg(feature = "video")]
#[test]
fn open_cpu_upload_without_device() {
    let cfg = tiny_h264();
    let enc = AutoVideoEncoder::open(&cfg);
    assert!(
        enc.is_ok(),
        "cpu upload open failed: {:?}",
        enc.as_ref().err()
    );
    if let Ok(enc) = enc {
        assert_eq!(enc.path_class(), EncodePathClass::CpuUpload);
        assert_eq!(enc.resolved_backend(), Backend::Os);
        let info = enc.stream_info();
        let geometry = info.geometry().unwrap_or(mediaway_common::VideoGeometry {
            width: 0,
            height: 0,
        });
        assert_eq!(geometry.width, 64);
        assert_eq!(geometry.height, 64);
    }
}

#[cfg(not(windows))]
#[cfg(feature = "video")]
#[test]
fn open_unsupported_off_windows() {
    let cfg = tiny_h264();
    assert!(matches!(
        AutoVideoEncoder::open(&cfg),
        Err(EncodeError::Unsupported)
    ));
}

#[test]
#[cfg(feature = "video")]
fn zero_copy_only_without_device_is_unsupported() {
    let mut cfg = tiny_h264();
    cfg.max_path_class = EncodePathClass::ZeroCopy;
    assert!(matches!(
        AutoVideoEncoder::open(&cfg),
        Err(EncodeError::Unsupported)
    ));
}

#[test]
#[cfg(feature = "video")]
fn invalid_dimensions() {
    let mut cfg = tiny_h264();
    cfg.width = 0;
    assert!(matches!(
        AutoVideoEncoder::open(&cfg),
        Err(EncodeError::InvalidInput)
    ));
}

/// `DirectX12` device present, but `max_path_class` doesn't reach `GpuCopy`: the match
/// guard on `GpuDeviceHandle::DirectX12` must fail closed (fall to the foreign-device
/// arm) rather than silently attempting a bridge the caller didn't opt into. Pure
/// decision-table check — never touches the (fake) pointer, so it needs no real
/// hardware and runs on every platform.
#[test]
#[cfg(feature = "video")]
fn directx12_without_gpu_copy_ceiling_is_unsupported() {
    let mut cfg = tiny_h264();
    let fake = NativeHandle::new(0x1234).expect("nonzero fake handle");
    cfg.gpu_device = Some(GpuDeviceHandle::DirectX12(fake));
    cfg.max_path_class = EncodePathClass::ZeroCopy;
    assert!(matches!(
        AutoVideoEncoder::open(&cfg),
        Err(EncodeError::Unsupported)
    ));
}

/// A GPU device kind this crate cannot bridge (Vulkan / Metal / `WebGpu`) must
/// not be silently dropped: `open` should record `Unsupported` for that arm
/// and still try the next tier (CPU upload) when `max_path_class` allows it. Real
/// MF open on Windows (mirrors `open_cpu_upload_without_device`); the fake
/// device handle is never dereferenced (foreign-device arm is a pure `_ =>`
/// match, no COM calls), so this stays memory-safe without real GPU hardware.
#[cfg(windows)]
#[cfg(feature = "video")]
#[test]
fn foreign_gpu_device_kind_falls_back_to_cpu_upload() {
    let mut cfg = tiny_h264();
    let fake = NativeHandle::new(0x1234).expect("nonzero fake handle");
    cfg.gpu_device = Some(GpuDeviceHandle::Vulkan(fake));
    let enc = AutoVideoEncoder::open(&cfg);
    assert!(
        enc.is_ok(),
        "expected cpu-upload fallback, got {:?}",
        enc.as_ref().err()
    );
    if let Ok(enc) = enc {
        assert_eq!(enc.path_class(), EncodePathClass::CpuUpload);
        assert_eq!(enc.gpu_copy_target(), None);
        assert_eq!(enc.gpu_copy_dx11_frame_handle(), None);
    }
}

// The end-to-end GpuCopy hardware test (`open_gpu_copy_via_d3d12_bridge_or_skip`)
// needs raw `unsafe` D3D12/DXGI calls to create a real device, which this
// module's `#![forbid(unsafe_code)]` (inherited by this `mod tests`) disallows.
// It lives in `crate::windows::tests` (`lib.rs`) instead, where Windows unsafe is already
// permitted for hardware-backed tests — see `d3d12_shared_bridge_open_or_skip`.

#[test]
#[cfg(feature = "video")]
fn av1_permissive_ceiling_falls_back_to_software() {
    use mediaway_common::Bytes;

    let mut cfg = AutoVideoEncodeConfig::new(CodecKind::Av1, 64, 64, Rational::new(1, 30));
    cfg.max_path_class = EncodePathClass::Software;
    let mut enc = AutoVideoEncoder::open(&cfg).expect("software AV1 fallback should open");
    assert_eq!(enc.path_class(), EncodePathClass::Software);
    assert_eq!(enc.resolved_backend(), Backend::Software);

    // I420 at 64x64: Y plane (64*64=4096) + U plane (32*32=1024) + V plane (32*32=1024)
    let y = vec![128u8; 64 * 64];
    let uv = vec![128u8; 32 * 32];
    let mut data = y;
    data.extend_from_slice(&uv);
    data.extend_from_slice(&uv);
    let frame = VideoFrame {
        pts: 0,
        duration: 1,
        width: 64,
        height: 64,
        format: PixelFormat::I420,
        storage: VideoFrameStorage::Cpu {
            data: Bytes::from(data),
        },
    };
    enc.push_frame(&frame).expect("push_frame");
    enc.flush().expect("flush");
    // draining is allowed to yield zero packets for one all-intra tiny frame
    // depending on rav1e's internal lookahead — just assert it doesn't error
    while let Ok(Some(_packet)) = enc.poll_packet() {}
}

/// `Explicit(Backend::Software)` must short-circuit straight to the software backend,
/// bypassing `max_path_class` entirely — `ZeroCopy` would normally reject every path
/// (see `zero_copy_only_without_device_is_unsupported`), but `backend` is checked first.
#[test]
#[cfg(feature = "video")]
fn explicit_software_bypasses_path_class_ceiling() {
    let mut cfg = AutoVideoEncodeConfig::new(CodecKind::Av1, 64, 64, Rational::new(1, 30));
    cfg.max_path_class = EncodePathClass::ZeroCopy;
    cfg.backend = BackendSelection::Explicit(Backend::Software);
    let enc = AutoVideoEncoder::open(&cfg);
    assert!(
        enc.is_ok(),
        "Explicit(Software) should open regardless of max_path_class: {:?}",
        enc.err()
    );
    if let Ok(enc) = enc {
        assert_eq!(enc.path_class(), EncodePathClass::Software);
        assert_eq!(enc.resolved_backend(), Backend::Software);
    }
}

/// `AutoHardwareOnly` tries NVENC, then `QuickSync`, then `Vulkan` — *ahead of* `Os`'s own
/// CPU upload — real hardware/driver dependent (this workspace's reference machine has all
/// three verified working, see `mediaway-encoder-nvenc`/`mediaway-encoder-quicksync`/
/// `mediaway-encoder-vulkan` `docs/roadmap.md`), so this skips honestly rather than failing
/// on a host without any of them. Without this ranking, `AutoHardwareOnly` would be
/// indistinguishable from `Auto` whenever `Os`'s own CPU upload already succeeds (as it does
/// on this reference machine) — see `open()`'s doc comment.
#[cfg(windows)]
#[cfg(feature = "video")]
#[test]
fn auto_hardware_only_tries_nvenc_then_quicksync_then_vulkan_or_skip() {
    let mut cfg = tiny_h264();
    cfg.backend = BackendSelection::AutoHardwareOnly;
    let enc = match AutoVideoEncoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: no NVENC/QuickSync/Vulkan GPU+driver available ({e:?})");
            return;
        }
    };
    assert_eq!(enc.path_class(), EncodePathClass::CpuUpload);
    if !matches!(
        enc.resolved_backend(),
        Backend::Nvenc | Backend::QuickSync | Backend::Vulkan
    ) {
        eprintln!(
            "skip: none of NVENC/QuickSync/Vulkan, AutoHardwareOnly fell back to {:?}",
            enc.resolved_backend()
        );
    }
}

/// `Explicit(Backend::Vulkan)` opens `mediaway-encoder::vulkan`'s `VK_KHR_video_encode_queue`
/// backend directly — real hardware/driver dependent, skips honestly. Uses 176x144, not
/// `tiny_h264()`'s 64x64: this driver's reported `minCodedExtent` for H.264 encode rejects
/// 64x64 (`EncodeError::InvalidInput`), matching the size `mediaway-encoder::vulkan`'s own
/// `encoder_tests.rs` already uses for exactly this reason.
#[cfg(windows)]
#[cfg(feature = "video")]
#[test]
fn explicit_vulkan_opens_or_skip() {
    let mut cfg = AutoVideoEncodeConfig::new(CodecKind::H264, 176, 144, Rational::new(1, 30));
    cfg.backend = BackendSelection::Explicit(Backend::Vulkan);
    let enc = match AutoVideoEncoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: no Vulkan Video encode queue available ({e:?})");
            return;
        }
    };
    assert_eq!(enc.path_class(), EncodePathClass::CpuUpload);
    assert_eq!(enc.resolved_backend(), Backend::Vulkan);
}

/// `Explicit(Backend::Amf)` always fails with `NoBackend` — no AMF implementation
/// exists in this crate yet (see `mediaway-encoder-amf` adr/0001).
#[test]
#[cfg(feature = "video")]
fn explicit_amf_reports_no_backend() {
    let mut cfg = tiny_h264();
    cfg.backend = BackendSelection::Explicit(Backend::Amf);
    assert!(matches!(
        AutoVideoEncoder::open(&cfg),
        Err(EncodeError::NoBackend)
    ));
}

#[cfg(windows)]
#[cfg(feature = "video")]
#[test]
fn support_probe_lists_every_backend() {
    let rows = super::support(CodecKind::H264);
    let backends: Vec<Backend> = rows.iter().map(|r| r.backend).collect();
    assert!(backends.contains(&Backend::Os));
    assert!(backends.contains(&Backend::Nvenc));
    assert!(backends.contains(&Backend::QuickSync));
    assert!(backends.contains(&Backend::Vulkan));
    assert!(backends.contains(&Backend::Amf));
    assert!(backends.contains(&Backend::Software));
}
