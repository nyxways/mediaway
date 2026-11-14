//! Tier-1 tests for the live decoder's bitstream gate (no GPU).
#![allow(clippy::unwrap_used, reason = "test module")]

use super::*;

#[test]
fn idr_gate_finds_an_idr_after_parameter_sets() {
    // AUD, SPS, PPS, IDR slice.
    let au = [
        0, 0, 0, 1, 0x09, 0xF0, 0, 0, 0, 1, 0x67, 0x64, 0, 0, 1, 0x68, 0xEE, 0, 0, 1, 0x65, 0x88,
    ];
    assert!(has_idr_slice(&au));
}

#[test]
fn idr_gate_stops_at_the_first_non_idr_slice() {
    // Non-IDR slice first: the scan must not look into slice data for an `00 00 01 65`.
    let au = [0, 0, 0, 1, 0x41, 0x9A, 0, 0, 1, 0x65, 0x88];
    assert!(!has_idr_slice(&au));
    assert!(!has_idr_slice(&[]));
    assert!(!has_idr_slice(&[0, 0, 1]));
}

#[test]
fn open_rejects_a_non_d3d11_device() {
    let cfg = WmfLiveH264Config::new(
        GpuDeviceHandle::DirectX12(NativeHandle::new(0x1000).unwrap()),
        1920,
        1080,
    );
    assert_eq!(
        WmfLiveH264Decoder::open(&cfg).err(),
        Some(DecodeError::Unsupported)
    );
}
