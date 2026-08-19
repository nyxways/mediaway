use super::*;
use mediaway_common::CodecKind;

fn tb(num: u64, den: u32) -> Rational {
    Rational::new(num, den)
}

#[test]
fn cmtime_from_ticks_num_one() {
    // time_base = 1/30 (common video framerate): ticks map straight to `value`.
    let (value, timescale) = cmtime_value_from_ticks(90, tb(1, 30));
    assert_eq!(value, 90);
    assert_eq!(timescale, 30);
}

#[test]
fn cmtime_from_ticks_arbitrary_num() {
    // time_base = 1001/30000 (NTSC-style): value must scale by num, not just pass ticks through.
    let (value, timescale) = cmtime_value_from_ticks(3, tb(1001, 30_000));
    assert_eq!(value, 3003);
    assert_eq!(timescale, 30_000);
}

#[test]
fn cmtime_from_ticks_negative() {
    let (value, timescale) = cmtime_value_from_ticks(-5, tb(1, 90_000));
    assert_eq!(value, -5);
    assert_eq!(timescale, 90_000);
}

#[test]
fn cmtime_round_trip_num_one() {
    let time_base = tb(1, 30);
    for ticks in [0_i64, 1, 30, 12_345, -7] {
        let (value, timescale) = cmtime_value_from_ticks(ticks, time_base);
        assert_eq!(ticks_from_cmtime_value(value, timescale, time_base), ticks);
    }
}

#[test]
fn cmtime_round_trip_arbitrary_num() {
    let time_base = tb(1001, 30_000);
    for ticks in [0_i64, 1, 100, 999_999] {
        let (value, timescale) = cmtime_value_from_ticks(ticks, time_base);
        assert_eq!(ticks_from_cmtime_value(value, timescale, time_base), ticks);
    }
}

#[test]
fn ticks_from_cmtime_zero_timescale_is_zero() {
    assert_eq!(ticks_from_cmtime_value(1000, 0, tb(1, 30)), 0);
}

#[test]
fn ticks_from_cmtime_zero_num_is_zero() {
    assert_eq!(ticks_from_cmtime_value(1000, 30, tb(0, 30)), 0);
}

#[test]
fn duration_from_cmtime_clamps_negative_to_zero() {
    assert_eq!(duration_ticks_from_cmtime_value(-100, 30, tb(1, 30)), 0);
}

#[test]
fn duration_from_cmtime_positive() {
    assert_eq!(duration_ticks_from_cmtime_value(30, 30, tb(1, 30)), 30);
}

#[test]
fn copy_nv12_planes_packs_tight_and_strips_stride() {
    // 2x2 luma with row stride 4 (2 padding bytes/row), 1x1 chroma (UV interleaved) row stride 4.
    let y_plane: [u8; 8] = [1, 2, 0xAA, 0xAA, 3, 4, 0xAA, 0xAA];
    let uv_plane: [u8; 4] = [9, 10, 0xAA, 0xAA];

    let out = copy_nv12_planes(&y_plane, 4, &uv_plane, 4, 2, 2);
    // width*height (4) luma bytes + width*height/2 (2) chroma bytes = 6 total.
    assert_eq!(out.as_ref(), &[1, 2, 3, 4, 9, 10]);
}

#[test]
fn copy_nv12_planes_truncated_input_zero_fills_rest() {
    // Only one luma row is actually available; the second should come back zeroed, not panic.
    let y_plane: [u8; 2] = [1, 2];
    let uv_plane: [u8; 2] = [9, 10];

    let out = copy_nv12_planes(&y_plane, 2, &uv_plane, 2, 2, 2);
    assert_eq!(out.as_ref(), &[1, 2, 0, 0, 9, 10]);
}

#[test]
fn copy_nv12_planes_empty_input_is_all_zero() {
    let out = copy_nv12_planes(&[], 2, &[], 2, 2, 2);
    assert_eq!(out.len(), 6);
    assert!(out.iter().all(|&b| b == 0));
}

#[test]
fn validate_parameter_sets_accepts_single_sps_pps_4_byte_length() {
    let config = AvcDecoderConfig {
        nal_length_size: 4,
        sps: vec![Bytes::from_static(&[0x67, 0x00])],
        pps: vec![Bytes::from_static(&[0x68, 0x00])],
    };
    assert!(validate_parameter_sets(&config).is_ok());
}

#[test]
fn validate_parameter_sets_rejects_non_4_byte_length() {
    let config = AvcDecoderConfig {
        nal_length_size: 2,
        sps: vec![Bytes::from_static(&[0x67])],
        pps: vec![Bytes::from_static(&[0x68])],
    };
    assert_eq!(
        validate_parameter_sets(&config),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn validate_parameter_sets_rejects_multiple_sps() {
    let config = AvcDecoderConfig {
        nal_length_size: 4,
        sps: vec![
            Bytes::from_static(&[0x67]),
            Bytes::from_static(&[0x67, 0x01]),
        ],
        pps: vec![Bytes::from_static(&[0x68])],
    };
    assert_eq!(
        validate_parameter_sets(&config),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn validate_parameter_sets_rejects_zero_pps() {
    let config = AvcDecoderConfig {
        nal_length_size: 4,
        sps: vec![Bytes::from_static(&[0x67])],
        pps: vec![],
    };
    assert_eq!(
        validate_parameter_sets(&config),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn is_supported_video_codec_h264_hevc_vp9_av1_prores() {
    assert!(is_supported_video_codec(CodecKind::H264));
    assert!(is_supported_video_codec(CodecKind::Hevc));
    assert!(is_supported_video_codec(CodecKind::Vp9));
    assert!(is_supported_video_codec(CodecKind::Av1));
    assert!(is_supported_video_codec(CodecKind::ProRes422Proxy));
    assert!(is_supported_video_codec(CodecKind::ProRes422Lt));
    assert!(is_supported_video_codec(CodecKind::ProRes422));
    assert!(is_supported_video_codec(CodecKind::ProRes422Hq));
    assert!(is_supported_video_codec(CodecKind::ProRes4444));
    assert!(is_supported_video_codec(CodecKind::ProRes4444Xq));
    assert!(!is_supported_video_codec(CodecKind::Opus));
}

#[test]
fn is_prores_only_the_six_prores_variants() {
    assert!(is_prores(CodecKind::ProRes422Proxy));
    assert!(is_prores(CodecKind::ProRes422Lt));
    assert!(is_prores(CodecKind::ProRes422));
    assert!(is_prores(CodecKind::ProRes422Hq));
    assert!(is_prores(CodecKind::ProRes4444));
    assert!(is_prores(CodecKind::ProRes4444Xq));
    assert!(!is_prores(CodecKind::H264));
    assert!(!is_prores(CodecKind::Hevc));
    assert!(!is_prores(CodecKind::Vp9));
    assert!(!is_prores(CodecKind::Av1));
}

#[test]
fn requires_extra_data_at_open_only_vp9_av1() {
    assert!(!requires_extra_data_at_open(CodecKind::H264));
    assert!(!requires_extra_data_at_open(CodecKind::Hevc));
    assert!(requires_extra_data_at_open(CodecKind::Vp9));
    assert!(requires_extra_data_at_open(CodecKind::Av1));
    assert!(!requires_extra_data_at_open(CodecKind::ProRes422));
}

#[test]
fn raw_atom_key_matches_codec() {
    assert_eq!(raw_atom_key(CodecKind::Vp9), Some("vpcC"));
    assert_eq!(raw_atom_key(CodecKind::Av1), Some("av1C"));
    assert_eq!(raw_atom_key(CodecKind::H264), None);
    assert_eq!(raw_atom_key(CodecKind::Hevc), None);
    assert_eq!(raw_atom_key(CodecKind::ProRes422), None);
}

#[test]
fn validate_hevc_parameter_sets_accepts_single_vps_sps_pps_4_byte_length() {
    let config = HevcDecoderConfig {
        nal_length_size: 4,
        vps: vec![Bytes::from_static(&[0x40, 0x01])],
        sps: vec![Bytes::from_static(&[0x42, 0x01])],
        pps: vec![Bytes::from_static(&[0x44, 0x01])],
    };
    assert!(validate_hevc_parameter_sets(&config).is_ok());
}

#[test]
fn validate_hevc_parameter_sets_rejects_non_4_byte_length() {
    let config = HevcDecoderConfig {
        nal_length_size: 2,
        vps: vec![Bytes::from_static(&[0x40])],
        sps: vec![Bytes::from_static(&[0x42])],
        pps: vec![Bytes::from_static(&[0x44])],
    };
    assert_eq!(
        validate_hevc_parameter_sets(&config),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn validate_hevc_parameter_sets_rejects_zero_vps() {
    let config = HevcDecoderConfig {
        nal_length_size: 4,
        vps: vec![],
        sps: vec![Bytes::from_static(&[0x42])],
        pps: vec![Bytes::from_static(&[0x44])],
    };
    assert_eq!(
        validate_hevc_parameter_sets(&config),
        Err(DecodeError::Unsupported)
    );
}
