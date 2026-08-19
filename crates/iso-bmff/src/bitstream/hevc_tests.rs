#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

/// A minimal but structurally real 15-byte SPS payload (NAL header + the fixed
/// `profile_tier_level` general fields this module reads), sized to exercise the "copy the
/// byte-aligned bytes" path in `build_hvcc`.
fn sample_sps() -> Vec<u8> {
    let mut sps = vec![0x42, 0x01]; // NAL header: nal_unit_type = 33 (SPS)
    sps.push(0x01); // sps_video_parameter_set_id / sps_max_sub_layers_minus1 / nesting
    sps.push(0x60); // general_profile_space/tier/profile_idc
    sps.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // profile_compatibility_flags
    sps.extend_from_slice(&[0x90, 0x00, 0x00, 0x00, 0x00, 0x00]); // constraint_indicator_flags
    sps.push(0x5a); // general_level_idc
    sps.extend_from_slice(&[0xaa, 0xbb]); // remaining bits, not read by this module
    sps
}

#[test]
fn annex_b_converts_when_vps_sps_pps_present() {
    let vps = [0x40u8, 0x01, 0x0c];
    let sps = sample_sps();
    let pps = [0x44u8, 0x01, 0xc0];

    let mut annex = vec![0u8, 0, 0, 1];
    annex.extend_from_slice(&vps);
    annex.extend_from_slice(&[0, 0, 0, 1]);
    annex.extend_from_slice(&sps);
    annex.extend_from_slice(&[0, 0, 0, 1]);
    annex.extend_from_slice(&pps);

    let out = to_hvcc(&annex);
    let hvcc = out.hvcc.expect("vps+sps+pps present");
    assert_eq!(hvcc[0], 1); // configurationVersion
    assert_eq!(hvcc[22], 3); // numOfArrays
}

#[test]
fn to_hvcc_passes_through_non_annex_b_input() {
    let already_framed = [0u8, 0, 0, 3, 0x40, 0x01, 0x0c];
    let out = to_hvcc(&already_framed);
    assert!(out.hvcc.is_none());
    assert_eq!(out.payload.as_ref(), &already_framed);
}

#[test]
fn build_hvcc_copies_byte_aligned_profile_tier_level_fields() {
    let vps = [0x40u8, 0x01];
    let sps = sample_sps();
    let pps = [0x44u8, 0x01];

    let hvcc = build_hvcc(&vps, &sps, &pps);
    assert_eq!(hvcc[1], sps[3]); // profile_space/tier/profile_idc
    assert_eq!(&hvcc[2..6], &sps[4..8]); // profile_compatibility_flags
    assert_eq!(&hvcc[6..12], &sps[8..14]); // constraint_indicator_flags
    assert_eq!(hvcc[12], sps[14]); // level_idc
}

#[test]
fn parse_hevc_decoder_config_recovers_vps_sps_pps_from_to_hvcc_output() {
    let vps = [0x40u8, 0x01, 0x0c];
    let sps = sample_sps();
    let pps = [0x44u8, 0x01, 0xc0];

    let mut annex = vec![0u8, 0, 0, 1];
    annex.extend_from_slice(&vps);
    annex.extend_from_slice(&[0, 0, 0, 1]);
    annex.extend_from_slice(&sps);
    annex.extend_from_slice(&[0, 0, 0, 1]);
    annex.extend_from_slice(&pps);

    let hvcc_record = to_hvcc(&annex).hvcc.expect("vps+sps+pps present");
    let config = parse_hevc_decoder_config(&hvcc_record).expect("valid hvcC record");

    assert_eq!(config.nal_length_size, 4);
    assert_eq!(config.vps, vec![Bytes::copy_from_slice(&vps)]);
    assert_eq!(config.sps, vec![Bytes::copy_from_slice(&sps)]);
    assert_eq!(config.pps, vec![Bytes::copy_from_slice(&pps)]);
}

#[test]
fn parse_hevc_decoder_config_rejects_truncated_record() {
    assert!(parse_hevc_decoder_config(&[1, 0, 0, 0]).is_none());
}

#[test]
fn parse_hevc_decoder_config_rejects_wrong_version_byte() {
    let mut record = vec![2u8];
    record.extend_from_slice(&[0u8; 22]);
    assert!(parse_hevc_decoder_config(&record).is_none());
}

#[test]
fn annex_b_sequence_header_concatenates_vps_sps_then_pps_with_start_codes() {
    let config = HevcDecoderConfig {
        nal_length_size: 4,
        vps: vec![Bytes::copy_from_slice(&[0x40, 0x01])],
        sps: vec![Bytes::copy_from_slice(&[0x42, 0xaa])],
        pps: vec![Bytes::copy_from_slice(&[0x44, 0xbb])],
    };
    let header = annex_b_sequence_header(&config);
    assert_eq!(
        header.as_ref(),
        &[
            0, 0, 0, 1, 0x40, 0x01, 0, 0, 0, 1, 0x42, 0xaa, 0, 0, 0, 1, 0x44, 0xbb
        ]
    );
}

#[test]
fn hvcc_payload_to_annex_b_inverts_to_hvcc_four_byte_lengths() {
    let vps = [0x40u8, 0x01, 0x0c];
    let sps = sample_sps();
    let pps = [0x44u8, 0x01, 0xc0];

    let mut annex = vec![0u8, 0, 0, 1];
    annex.extend_from_slice(&vps);
    annex.extend_from_slice(&[0, 0, 0, 1]);
    annex.extend_from_slice(&sps);
    annex.extend_from_slice(&[0, 0, 0, 1]);
    annex.extend_from_slice(&pps);

    let payload = to_hvcc(&annex).payload;
    let back = hvcc_payload_to_annex_b(&payload, 4);
    assert_eq!(back.as_ref(), &annex);
}

#[test]
fn hvcc_payload_to_annex_b_stops_cleanly_on_truncated_length() {
    let truncated = [0u8, 0, 0];
    let out = hvcc_payload_to_annex_b(&truncated, 4);
    assert!(out.is_empty());
}
