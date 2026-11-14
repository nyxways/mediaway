//! Config validation for [`NvencLiveEncoder`] that needs no GPU (Tier 1).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;
use mediaway_common::NativeHandle;

fn d3d12_device() -> GpuDeviceHandle {
    GpuDeviceHandle::DirectX12(NativeHandle::new(0x1000).unwrap())
}

#[test]
fn new_sets_live_defaults() {
    let cfg = NvencLiveConfig::new(d3d12_device(), 1920, 1080, 60, 1, 10_000_000);
    assert_eq!(cfg.preset, NvencPreset::P4);
    assert_eq!(cfg.latency, NvencLatency::UltraLow);
    assert_eq!(cfg.input, NvencLiveInput::Bgra);
    assert_eq!(cfg.vbv_buffer_bits, 0);
    assert_eq!(cfg.intra_refresh, None);
}

#[test]
fn open_rejects_bad_geometry_before_touching_the_device() {
    let mut cfg = NvencLiveConfig::new(d3d12_device(), 1921, 1080, 60, 1, 10_000_000);
    let expected = if cfg!(windows) {
        EncodeError::InvalidInput
    } else {
        EncodeError::Unsupported
    };
    assert_eq!(NvencLiveEncoder::open(&cfg).err(), Some(expected.clone()));
    cfg.width = 1920;
    cfg.max_width = 1280;
    assert_eq!(NvencLiveEncoder::open(&cfg).err(), Some(expected.clone()));
    cfg.max_width = 0;
    cfg.intra_refresh = Some(IntraRefresh {
        period_frames: 30,
        duration_frames: 60,
    });
    assert_eq!(NvencLiveEncoder::open(&cfg).err(), Some(expected));
}

#[test]
fn open_rejects_a_non_d3d11_device() {
    let cfg = NvencLiveConfig::new(d3d12_device(), 1920, 1080, 60, 1, 10_000_000);
    assert_eq!(
        NvencLiveEncoder::open(&cfg).err(),
        Some(EncodeError::Unsupported)
    );
}
