#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

#[test]
fn annex_b_converts_when_four_byte_start_codes_present() {
    let annex = [
        0u8, 0, 0, 1, 0x67, 0x42, 0x00, 0x1e, 0xaa, 0, 0, 0, 1, 0x68, 0xce, 0x06, 0xe2, 0, 0, 0, 1,
        0x65, 0x88,
    ];
    let out = to_avcc(&annex);
    assert!(out.avcc.is_some());
    assert_eq!(&out.payload[..4], &5u32.to_be_bytes());
}

#[test]
fn annex_b_converts_when_three_byte_start_codes_present() {
    let annex = [0u8, 0, 1, 0x67, 0x42, 0x00, 0x1e, 0, 0, 1, 0x68, 0xce];
    let out = to_avcc(&annex);
    assert!(out.avcc.is_some());
    assert_eq!(&out.payload[..4], &4u32.to_be_bytes());
}

#[test]
fn parse_avc_decoder_config_recovers_sps_pps_from_to_avcc_output() {
    let sps = [0x67u8, 0x42, 0x00, 0x1e, 0xaa];
    let pps = [0x68u8, 0xce, 0x06, 0xe2];
    let mut annex = vec![0u8, 0, 0, 1];
    annex.extend_from_slice(&sps);
    annex.extend_from_slice(&[0, 0, 0, 1]);
    annex.extend_from_slice(&pps);

    let out = to_avcc(&annex);
    let avcc_record = out.avcc.expect("sps+pps present");

    let config = parse_avc_decoder_config(&avcc_record).expect("valid avcC record");
    assert_eq!(config.nal_length_size, 4);
    assert_eq!(config.sps, vec![Bytes::copy_from_slice(&sps)]);
    assert_eq!(config.pps, vec![Bytes::copy_from_slice(&pps)]);
}

#[test]
fn annex_b_sequence_header_concatenates_sps_then_pps_with_start_codes() {
    let config = AvcDecoderConfig {
        nal_length_size: 4,
        sps: vec![Bytes::copy_from_slice(&[0x67, 0xaa])],
        pps: vec![Bytes::copy_from_slice(&[0x68, 0xbb])],
    };
    let header = annex_b_sequence_header(&config);
    assert_eq!(
        header.as_ref(),
        &[0, 0, 0, 1, 0x67, 0xaa, 0, 0, 0, 1, 0x68, 0xbb]
    );
}

#[test]
fn parse_avc_decoder_config_rejects_truncated_record() {
    assert!(parse_avc_decoder_config(&[1, 0x42, 0, 0x1e]).is_none());
}

#[test]
fn parse_avc_decoder_config_rejects_wrong_version_byte() {
    let mut record = vec![2u8, 0x42, 0x00, 0x1e, 0xff, 0xe1, 0, 2, 0xaa, 0xbb];
    record.push(0);
    assert!(parse_avc_decoder_config(&record).is_none());
}

#[test]
fn avcc_payload_to_annex_b_inverts_to_avcc_four_byte_lengths() {
    let annex = [
        0u8, 0, 0, 1, 0x67, 0x42, 0x00, 0x1e, 0xaa, 0, 0, 0, 1, 0x68, 0xce, 0x06, 0xe2, 0, 0, 0, 1,
        0x65, 0x88,
    ];
    let avcc = to_avcc(&annex).payload;
    let back = avcc_payload_to_annex_b(&avcc, 4);
    assert_eq!(back.as_ref(), &annex);
}

#[test]
fn avcc_payload_to_annex_b_stops_cleanly_on_truncated_length() {
    let truncated = [0u8, 0, 0]; // shorter than the 4-byte length prefix it claims
    let out = avcc_payload_to_annex_b(&truncated, 4);
    assert!(out.is_empty());
}
