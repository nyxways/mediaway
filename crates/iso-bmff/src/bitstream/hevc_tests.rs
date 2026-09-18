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

/// The escape-free case, where the NAL as transmitted and its RBSP are the same bytes.
///
/// `sample_sps()` deliberately contains no `00 00 03` sequence, so this asserts the offsets
/// and nothing about unescaping. **That is why it passed for a year while `build_hvcc` was
/// wrong on every real SPS** — see `build_hvcc_unescapes_a_real_encoder_sps` below, which is
/// the test this one should always have been paired with.
#[test]
fn build_hvcc_copies_byte_aligned_profile_tier_level_fields_when_nothing_is_escaped() {
    let vps = [0x40u8, 0x01];
    let sps = sample_sps();
    let pps = [0x44u8, 0x01];
    assert!(
        !sps.windows(3).any(|w| w == [0x00, 0x00, 0x03]),
        "this fixture must stay escape-free for the test to mean what it says"
    );

    let hvcc = build_hvcc(&vps, &sps, &pps);
    assert_eq!(hvcc[1], sps[3]); // profile_space/tier/profile_idc
    assert_eq!(&hvcc[2..6], &sps[4..8]); // profile_compatibility_flags
    assert_eq!(&hvcc[6..12], &sps[8..14]); // constraint_indicator_flags
    assert_eq!(hvcc[12], sps[14]); // level_idc
}

/// A real SPS emitted by the NVIDIA HEVC Encoder MFT, captured 2026-09-18 on an RTX 4090.
///
/// Truncated after `general_level_idc` plus a few bytes: everything `build_hvcc` reads lives
/// in the first 15 RBSP bytes, and the tail is exp-golomb-coded fields this module does not
/// touch. The three `00 00 03` escapes inside `profile_tier_level()` are the point of the
/// fixture — a Main-profile SPS is zero-heavy exactly there.
fn real_nvidia_sps() -> Vec<u8> {
    vec![
        0x42, 0x01, // NAL header, nal_unit_type = 33 (SPS)
        0x01, // sps_video_parameter_set_id=0 / max_sub_layers_minus1=0 / temporal_id_nesting=1
        0x01, // general_profile_space=0, tier=0, profile_idc=1 (Main)
        0x60, 0x00, 0x00, 0x03, 0x00, // compatibility flags, with an escape
        0x90, 0x00, 0x00, 0x03, 0x00, 0x00, 0x03, 0x00, // constraint flags, two more escapes
        0x5a, // general_level_idc = 90 -> level 3.0
        0xa0, 0x05, 0x02, 0x01, 0xe1, // tail, not read here
    ]
}

/// The whole reason this module was rewritten.
///
/// Every expected byte is cross-checked against what **ffmpeg** writes into `hvcC` for the
/// same encoder and the same sequence header. Before the fix, six of these were wrong —
/// `general_level_idc` among them, reading `0x00` instead of `0x5a`, which is a record
/// claiming "level unspecified" for a level-3.0 stream.
#[test]
fn build_hvcc_unescapes_a_real_encoder_sps() {
    let vps = [0x40u8, 0x01, 0x0c];
    let sps = real_nvidia_sps();
    let pps = [0x44u8, 0x01, 0xc0];

    let hvcc = build_hvcc(&vps, &sps, &pps);

    assert_eq!(hvcc[0], 1, "configurationVersion");
    assert_eq!(hvcc[1], 0x01, "general_profile_space/tier/profile_idc");
    assert_eq!(
        &hvcc[2..6],
        &[0x60, 0x00, 0x00, 0x00],
        "general_profile_compatibility_flags: the escape at RBSP byte 7 shifted this"
    );
    assert_eq!(
        &hvcc[6..12],
        &[0x90, 0x00, 0x00, 0x00, 0x00, 0x00],
        "general_constraint_indicator_flags: three escapes fall inside this range"
    );
    assert_eq!(
        hvcc[12], 0x5a,
        "general_level_idc must survive as level 3.0, not read as 0"
    );
    assert_eq!(
        hvcc[21], 0x0f,
        "constantFrameRate=0 | numTemporalLayers=1 | temporalIdNested=1 | lengthSizeMinusOne=3"
    );
    assert_eq!(hvcc[22], 3, "numOfArrays: VPS, SPS, PPS");
}

/// The parameter-set arrays carry the NAL **as transmitted**, escapes intact.
///
/// Unescaping them would corrupt the parameter sets a decoder reads back out — the RBSP is
/// only ever for lifting header fields out of.
#[test]
fn build_hvcc_stores_parameter_sets_still_escaped() {
    let vps = [0x40u8, 0x01, 0x0c];
    let sps = real_nvidia_sps();
    let pps = [0x44u8, 0x01, 0xc0];

    let hvcc = build_hvcc(&vps, &sps, &pps);
    let config = parse_hevc_decoder_config(&hvcc).expect("valid hvcC record");
    assert_eq!(config.sps, vec![Bytes::copy_from_slice(&sps)]);
    assert_eq!(config.vps, vec![Bytes::copy_from_slice(&vps)]);
    assert_eq!(config.pps, vec![Bytes::copy_from_slice(&pps)]);
}

#[test]
fn unescape_rbsp_removes_only_the_escape_byte() {
    assert_eq!(
        unescape_rbsp(&[0x42, 0x00, 0x00, 0x03, 0x01]),
        vec![0x42, 0x00, 0x00, 0x01]
    );
}

#[test]
fn unescape_rbsp_is_identity_without_escapes() {
    let input = [0x42u8, 0x01, 0x60, 0x00, 0x01, 0xff];
    assert_eq!(unescape_rbsp(&input), input.to_vec());
}

#[test]
fn unescape_rbsp_does_not_chain_one_escape_into_the_next() {
    // After dropping an escape the zero run restarts, so `00 00 03 00 00 03` is two separate
    // escapes and not one escape plus a spurious match.
    assert_eq!(
        unescape_rbsp(&[0x00, 0x00, 0x03, 0x00, 0x00, 0x03, 0x01]),
        vec![0x00, 0x00, 0x00, 0x00, 0x01]
    );
}

#[test]
fn temporal_layer_byte_reads_sub_layers_and_nesting_from_the_sps() {
    // sps_max_sub_layers_minus1 = 2 (three layers), temporal_id_nesting_flag = 0.
    let rbsp = [0x42u8, 0x01, 0b0000_0100];
    assert_eq!(temporal_layer_byte(&rbsp), (3 << 3) | 0x03);
}

#[test]
fn temporal_layer_byte_falls_back_to_one_unnested_layer() {
    assert_eq!(temporal_layer_byte(&[0x42, 0x01]), (1 << 3) | 0x03);
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
