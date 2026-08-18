#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;
use mediaway_common::CodecKind;

#[test]
fn baseline_profile_idc_yields_constrained_baseline_only() {
    let candidates = h264_profile_candidates(66).expect("baseline supported");
    assert_eq!(
        candidates,
        vec![cros_libva::VAProfile::VAProfileH264ConstrainedBaseline]
    );
}

#[test]
fn main_profile_idc_prefers_main_then_falls_back_to_constrained_baseline() {
    let candidates = h264_profile_candidates(77).expect("main supported");
    assert_eq!(
        candidates,
        vec![
            cros_libva::VAProfile::VAProfileH264Main,
            cros_libva::VAProfile::VAProfileH264ConstrainedBaseline,
        ]
    );
}

#[test]
fn high_profile_idc_is_unsupported_this_session() {
    assert_eq!(h264_profile_candidates(100), Err(DecodeError::Unsupported));
}

#[test]
fn h264_and_av1_codec_kinds_are_supported() {
    assert!(is_supported_video_codec(CodecKind::H264));
    assert!(is_supported_video_codec(CodecKind::Av1));
    assert!(!is_supported_video_codec(CodecKind::Hevc));
    assert!(!is_supported_video_codec(CodecKind::Vp9));
}

#[test]
fn av1_profile_candidates_is_main_profile_only() {
    assert_eq!(
        av1_profile_candidates(),
        vec![cros_libva::VAProfile::VAProfileAV1Profile0]
    );
}
