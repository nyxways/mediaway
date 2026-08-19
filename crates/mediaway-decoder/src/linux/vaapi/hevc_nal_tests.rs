#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

/// Builds a 2-byte HEVC NAL header for `nal_unit_type` with `nuh_layer_id = 0`,
/// `nuh_temporal_id_plus1 = 1`.
fn nal_header(nal_unit_type: u8) -> [u8; 2] {
    let first = nal_unit_type << 1;
    let second = 0x01; // nuh_layer_id (5 low bits) = 0, nuh_temporal_id_plus1 (3 bits) = 1
    [first, second]
}

#[test]
fn parse_maps_known_nal_unit_types() {
    for (raw, expected) in [
        (0u8, HevcNalUnitType::Trail),
        (1u8, HevcNalUnitType::Trail),
        (19u8, HevcNalUnitType::Idr),
        (20u8, HevcNalUnitType::Idr),
        (21u8, HevcNalUnitType::Cra),
        (32u8, HevcNalUnitType::Vps),
        (33u8, HevcNalUnitType::Sps),
        (34u8, HevcNalUnitType::Pps),
        (39u8, HevcNalUnitType::Other(39)),
    ] {
        let mut data = nal_header(raw).to_vec();
        data.push(0xAB);
        let unit = HevcNalUnit::parse(&data).unwrap();
        assert_eq!(unit.unit_type, expected);
    }
}

#[test]
fn parse_marks_trail_r_and_idr_as_reference_trail_n_as_not() {
    let mut trail_r = nal_header(1).to_vec(); // TRAIL_R
    trail_r.push(0x00);
    assert!(HevcNalUnit::parse(&trail_r).unwrap().is_reference);

    let mut trail_n = nal_header(0).to_vec(); // TRAIL_N
    trail_n.push(0x00);
    assert!(!HevcNalUnit::parse(&trail_n).unwrap().is_reference);

    let mut idr = nal_header(19).to_vec(); // IDR_W_RADL
    idr.push(0x00);
    assert!(HevcNalUnit::parse(&idr).unwrap().is_reference);
}

#[test]
fn parse_rejects_data_shorter_than_two_bytes() {
    assert_eq!(HevcNalUnit::parse(&[0x26]), Err(DecodeError::InvalidInput));
    assert_eq!(HevcNalUnit::parse(&[]), Err(DecodeError::InvalidInput));
}

#[test]
fn parse_rejects_nonzero_nuh_layer_id() {
    let mut data = nal_header(19).to_vec();
    data[1] |= 0x08; // set a nuh_layer_id low bit
    data.push(0x00);
    assert_eq!(HevcNalUnit::parse(&data), Err(DecodeError::Unsupported));
}

#[test]
fn rbsp_strips_two_byte_header() {
    let mut data = nal_header(1).to_vec();
    data.extend_from_slice(&[0x11, 0x22, 0x33]);
    let unit = HevcNalUnit::parse(&data).unwrap();
    assert_eq!(unit.rbsp, vec![0x11, 0x22, 0x33]);
}

#[test]
fn rbsp_removes_emulation_prevention_three_byte() {
    let mut data = nal_header(1).to_vec();
    // 00 00 03 01 -> emulation-prevention byte removed -> 00 00 01
    data.extend_from_slice(&[0x00, 0x00, 0x03, 0x01]);
    let unit = HevcNalUnit::parse(&data).unwrap();
    assert_eq!(unit.rbsp, vec![0x00, 0x00, 0x01]);
}

#[test]
fn rbsp_does_not_strip_0x03_outside_emulation_run() {
    let mut data = nal_header(1).to_vec();
    data.extend_from_slice(&[0x01, 0x03, 0x02]);
    let unit = HevcNalUnit::parse(&data).unwrap();
    assert_eq!(unit.rbsp, vec![0x01, 0x03, 0x02]);
}
