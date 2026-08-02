#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

#[test]
fn split_annex_b_finds_single_nal_with_3_byte_start_code() {
    // 00 00 01 <header=0x67 (SPS)> <payload bytes>
    let data = [0x00, 0x00, 0x01, 0x67, 0xAA, 0xBB];
    let units = split_annex_b(&data).unwrap();
    assert_eq!(units, vec![[0x67, 0xAA, 0xBB].as_slice()]);
}

#[test]
fn split_annex_b_finds_single_nal_with_4_byte_start_code() {
    // 00 00 00 01 <header=0x68 (PPS)> <payload bytes>
    let data = [0x00, 0x00, 0x00, 0x01, 0x68, 0xCC];
    let units = split_annex_b(&data).unwrap();
    assert_eq!(units, vec![[0x68, 0xCC].as_slice()]);
}

#[test]
fn split_annex_b_splits_multiple_nal_units_mixed_start_code_lengths() {
    let data = [
        0x00, 0x00, 0x01, 0x67, 0xAA, // NAL 1 via 3-byte start code
        0x00, 0x00, 0x00, 0x01, 0x68, 0xBB, 0xCC, // NAL 2 via 4-byte start code
        0x00, 0x00, 0x01, 0x65, 0xDD, // NAL 3 via 3-byte start code
    ];
    let units = split_annex_b(&data).unwrap();
    assert_eq!(
        units,
        vec![
            [0x67, 0xAA].as_slice(),
            [0x68, 0xBB, 0xCC].as_slice(),
            [0x65, 0xDD].as_slice(),
        ]
    );
}

#[test]
fn split_annex_b_errors_when_no_start_code_present() {
    let data = [0x67, 0xAA, 0xBB, 0xCC];
    assert_eq!(split_annex_b(&data), Err(H264Error::NoStartCode));
}

#[test]
fn split_annex_b_trims_trailing_zero_padding_before_next_start_code() {
    // NAL 1 payload ends with real zero bytes, then a 4-byte start code begins NAL 2.
    // The extra leading 0x00 of the 4-byte code must not leak into NAL 1's payload.
    let data = [
        0x00, 0x00, 0x01, 0x67, 0x00, 0x00, // NAL 1: header + two "real" trailing zeros
        0x00, 0x00, 0x00, 0x01, 0x68, // NAL 2 via 4-byte start code
    ];
    let units = split_annex_b(&data).unwrap();
    assert_eq!(units, vec![[0x67].as_slice(), [0x68].as_slice()]);
}

#[test]
fn nal_unit_parse_decodes_ref_idc_and_unit_type() {
    // header 0x67 = 0110_0111: forbidden_zero_bit=0, nal_ref_idc=0b11=3, nal_unit_type=0b00111=7 (SPS)
    let data = [0x67, 0x01, 0x02];
    let nal = NalUnit::parse(&data).unwrap();
    assert_eq!(nal.ref_idc, 3);
    assert_eq!(nal.unit_type, NalUnitType::Sps);
    assert_eq!(nal.rbsp.as_ref(), [0x01, 0x02].as_slice());
}

#[test]
fn nal_unit_parse_decodes_non_ref_slice_type() {
    // header 0x41 = 0100_0001: nal_ref_idc=0b10=2, nal_unit_type=0b00001=1 (non-IDR slice)
    let data = [0x41, 0xFF];
    let nal = NalUnit::parse(&data).unwrap();
    assert_eq!(nal.ref_idc, 2);
    assert_eq!(nal.unit_type, NalUnitType::NonIdrSlice);
}

#[test]
fn nal_unit_parse_removes_emulation_prevention_byte() {
    // header 0x67, then RBSP bytes 0x00 0x00 0x03 0x01 -> emulation-prevention 0x03 removed -> 0x00 0x00 0x01
    let data = [0x67, 0x00, 0x00, 0x03, 0x01];
    let nal = NalUnit::parse(&data).unwrap();
    assert_eq!(nal.rbsp.as_ref(), [0x00, 0x00, 0x01].as_slice());
}

#[test]
fn nal_unit_parse_keeps_0x03_when_not_after_two_zero_bytes() {
    let data = [0x67, 0x01, 0x03, 0x02];
    let nal = NalUnit::parse(&data).unwrap();
    assert_eq!(nal.rbsp.as_ref(), [0x01, 0x03, 0x02].as_slice());
}

#[test]
fn nal_unit_parse_errors_on_empty_input() {
    assert_eq!(NalUnit::parse(&[]), Err(H264Error::UnexpectedEof));
}

#[test]
fn split_avcc_splits_length_prefixed_nal_units() {
    let data = [
        0x00, 0x00, 0x00, 0x02, 0x67, 0xAA, // length=2
        0x00, 0x00, 0x00, 0x01, 0x68, // length=1
    ];
    let units = split_avcc(&data, 4).unwrap();
    assert_eq!(units, vec![[0x67, 0xAA].as_slice(), [0x68].as_slice()]);
}

#[test]
fn split_avcc_supports_2_byte_length_prefix() {
    let data = [0x00, 0x03, 0x67, 0xAA, 0xBB];
    let units = split_avcc(&data, 2).unwrap();
    assert_eq!(units, vec![[0x67, 0xAA, 0xBB].as_slice()]);
}

#[test]
fn split_avcc_errors_when_declared_length_exceeds_remaining_data() {
    let data = [0x00, 0x00, 0x00, 0xFF, 0x67]; // declares 255 bytes, only 1 remains
    assert_eq!(split_avcc(&data, 4), Err(H264Error::InvalidNalLength));
}

#[test]
fn split_avcc_errors_on_truncated_length_prefix() {
    let data = [0x00, 0x00]; // shorter than the 4-byte length prefix
    assert_eq!(split_avcc(&data, 4), Err(H264Error::InvalidNalLength));
}

#[test]
fn split_avcc_errors_on_invalid_length_size() {
    let data = [0x00, 0x00, 0x00, 0x01, 0x67];
    assert_eq!(split_avcc(&data, 0), Err(H264Error::InvalidLengthSize));
    assert_eq!(split_avcc(&data, 5), Err(H264Error::InvalidLengthSize));
}
