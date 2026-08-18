#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

/// `nal_unit_type = 7` (SPS), `nal_ref_idc = 3`: `0b011_00111 = 0x67`.
const SPS_HEADER: u8 = 0x67;
/// `nal_unit_type = 8` (PPS), `nal_ref_idc = 3`: `0b011_01000 = 0x68`.
const PPS_HEADER: u8 = 0x68;
/// `nal_unit_type = 1` (non-IDR slice), `nal_ref_idc = 0`: `0b000_00001 = 0x01`.
const SLICE_HEADER: u8 = 0x01;

#[test]
fn prepend_start_code_adds_canonical_four_byte_marker() {
    let nal = [SPS_HEADER, 0xAA, 0xBB];
    let out = prepend_start_code(&nal);
    assert_eq!(out, vec![0x00, 0x00, 0x00, 0x01, SPS_HEADER, 0xAA, 0xBB]);
}

#[test]
fn splits_sps_and_pps_and_prepends_start_codes() {
    // Annex-B: 00 00 00 01 <SPS NAL> 00 00 00 01 <PPS NAL>
    let extra_data = [
        0x00, 0x00, 0x00, 0x01, SPS_HEADER, 0xAA, 0xBB, //
        0x00, 0x00, 0x00, 0x01, PPS_HEADER, 0xCC,
    ];

    let (sps, pps) = split_csd(&extra_data);

    assert_eq!(
        sps.expect("SPS NAL present in extra_data"),
        vec![0x00, 0x00, 0x00, 0x01, SPS_HEADER, 0xAA, 0xBB]
    );
    assert_eq!(
        pps.expect("PPS NAL present in extra_data"),
        vec![0x00, 0x00, 0x00, 0x01, PPS_HEADER, 0xCC]
    );
}

#[test]
fn only_first_sps_and_pps_are_kept() {
    let extra_data = [
        0x00, 0x00, 0x00, 0x01, SPS_HEADER, 0x01, //
        0x00, 0x00, 0x00, 0x01, SPS_HEADER, 0x02, // second SPS: ignored
        0x00, 0x00, 0x00, 0x01, PPS_HEADER, 0x03, //
        0x00, 0x00, 0x00, 0x01, PPS_HEADER, 0x04, // second PPS: ignored
    ];

    let (sps, pps) = split_csd(&extra_data);

    assert_eq!(
        sps.expect("first SPS present"),
        vec![0x00, 0x00, 0x00, 0x01, SPS_HEADER, 0x01]
    );
    assert_eq!(
        pps.expect("first PPS present"),
        vec![0x00, 0x00, 0x00, 0x01, PPS_HEADER, 0x03]
    );
}

#[test]
fn non_parameter_set_nals_are_ignored() {
    let extra_data = [0x00, 0x00, 0x00, 0x01, SLICE_HEADER, 0xEE, 0xFF];

    let (sps, pps) = split_csd(&extra_data);
    assert!(sps.is_none());
    assert!(pps.is_none());
}

#[test]
fn extra_data_with_no_start_code_yields_no_csd() {
    let extra_data = [SPS_HEADER, 0xAA, 0xBB];

    let (sps, pps) = split_csd(&extra_data);
    assert!(sps.is_none());
    assert!(pps.is_none());
}

#[test]
fn empty_extra_data_yields_no_csd() {
    let (sps, pps) = split_csd(&[]);
    assert!(sps.is_none());
    assert!(pps.is_none());
}
