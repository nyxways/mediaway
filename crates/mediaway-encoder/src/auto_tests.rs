//! Unit tests for auto encode types (session constructors live in platform crates).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::{AutoVideoEncodeConfig, Backend, BackendSelection, EncodePathClass};
use crate::VideoInputPreference;
use mediaway_common::{CodecKind, GpuDeviceHandle, NativeHandle, Rational};

#[test]
fn path_class_labels_match_bench_vocab() {
    assert_eq!(EncodePathClass::ZeroCopy.label(), "zc");
    assert_eq!(EncodePathClass::GpuCopy.label(), "copy");
    assert_eq!(EncodePathClass::CpuUpload.label(), "upload");
    assert_eq!(EncodePathClass::Readback.label(), "readback");
    assert_eq!(EncodePathClass::Software.label(), "sw");
}

/// `max_path_class` is a **ceiling**, not a set: tolerance nests monotonically, so
/// ordering (not bitflags) is what `mediaway-encoder-windows::auto::open` compares
/// against.
#[test]
fn path_class_ordering_is_cheapest_first() {
    assert!(EncodePathClass::ZeroCopy < EncodePathClass::GpuCopy);
    assert!(EncodePathClass::GpuCopy < EncodePathClass::CpuUpload);
    assert!(EncodePathClass::CpuUpload < EncodePathClass::Readback);
    assert!(EncodePathClass::Readback < EncodePathClass::Software);
}

#[test]
fn new_takes_explicit_size() {
    let cfg = AutoVideoEncodeConfig::new(CodecKind::H264, 64, 64, Rational::new(1, 30));
    assert_eq!(cfg.backend, BackendSelection::Auto);
    assert_eq!(cfg.max_path_class, EncodePathClass::CpuUpload);
    assert_eq!(cfg.width, 64);
    assert_eq!(cfg.height, 64);
    assert_eq!(cfg.gpu_device, None);
}

/// `Explicit` names one backend directly — `Backend` carries no fallback/ranking
/// semantics of its own, that's entirely `BackendSelection`'s job.
#[test]
fn explicit_selection_names_one_backend() {
    let sel = BackendSelection::Explicit(Backend::Nvenc);
    assert_eq!(sel, BackendSelection::Explicit(Backend::Nvenc));
    assert_ne!(sel, BackendSelection::Explicit(Backend::QuickSync));
}

/// `to_low_level` must carry a `DirectX12` device through unchanged: the
/// Windows `GpuCopy` branch reads `config.gpu_device` to decide whether to
/// bridge via `D3d12SharedEncodeBridge` *before* ever building a low-level
/// config — so the facade config has to preserve whichever `GpuDeviceHandle`
/// variant the caller passed, not just `DirectX11`.
#[test]
fn to_low_level_preserves_directx12_device() {
    let cfg = AutoVideoEncodeConfig::new(CodecKind::H264, 64, 64, Rational::new(1, 30));
    let handle = NativeHandle::new(0x1000).expect("nonzero handle");
    let low = cfg.to_low_level(
        VideoInputPreference::ZeroCopyGpu,
        Some(GpuDeviceHandle::DirectX12(handle)),
    );
    assert_eq!(low.gpu_device, Some(GpuDeviceHandle::DirectX12(handle)));
}
