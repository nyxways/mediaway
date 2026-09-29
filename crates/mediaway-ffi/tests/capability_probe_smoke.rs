//! Integration: `mediaway_encoder_support_at` / `mediaway_decoder_support` through the C ABI
//! (`adr/pipeline/0007-stream-bytes-aac-decode-support-probe.md` §3).
//!
//! The probes open real throwaway sessions, so the exact rows depend on the machine. What is
//! asserted is the contract: argument rules, the flattened row invariants, ownership, and that
//! the answers agree with what actually opening an encoder / decoder does.

#![cfg(feature = "pipeline")]
#![allow(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::panic,
    reason = "integration test"
)]

use mediaway_ffi::pipeline::{
    MediawayEncodePathClass, MediawayEncoderCapability, MediawayPipelineCodecKind,
    MediawayPipelineStatus, MediawaySupportState, mediaway_decoder_support,
    mediaway_encoder_support_at, mediaway_encoder_support_free,
};

fn encoder_rows(
    codec: MediawayPipelineCodecKind,
    w: u32,
    h: u32,
) -> Vec<MediawayEncoderCapability> {
    let mut rows = std::ptr::null_mut();
    let mut count = 99usize;
    let status = unsafe { mediaway_encoder_support_at(codec, w, h, &raw mut rows, &raw mut count) };
    assert_eq!(status, MediawayPipelineStatus::Ok);
    if count == 0 {
        assert!(rows.is_null(), "zero rows must be NULL");
        return Vec::new();
    }
    // SAFETY: `rows`/`count` were just written by the probe.
    let out = unsafe { std::slice::from_raw_parts(rows, count) }.to_vec();
    unsafe { mediaway_encoder_support_free(rows, count) };
    out
}

#[test]
fn encoder_probe_argument_rules() {
    let mut rows = std::ptr::null_mut();
    let mut count = 0usize;
    let h264 = MediawayPipelineCodecKind::H264;
    assert_eq!(
        unsafe { mediaway_encoder_support_at(h264, 64, 64, std::ptr::null_mut(), &raw mut count) },
        MediawayPipelineStatus::InvalidArgument
    );
    assert_eq!(
        unsafe { mediaway_encoder_support_at(h264, 64, 64, &raw mut rows, std::ptr::null_mut()) },
        MediawayPipelineStatus::InvalidArgument
    );
    // Zero dimensions are refused, and the out-parameters are left cleared.
    count = 5;
    let status = unsafe { mediaway_encoder_support_at(h264, 0, 64, &raw mut rows, &raw mut count) };
    assert_eq!(status, MediawayPipelineStatus::InvalidInput);
    assert!(rows.is_null() && count == 0);
    // Freeing (NULL, 0) is always safe.
    unsafe { mediaway_encoder_support_free(std::ptr::null_mut(), 0) };
}

#[test]
fn encoder_rows_keep_their_invariants() {
    for codec in [
        MediawayPipelineCodecKind::H264,
        MediawayPipelineCodecKind::Hevc,
    ] {
        for row in encoder_rows(codec, 1280, 720) {
            match row.state {
                MediawaySupportState::Supported => assert_ne!(
                    row.path_class,
                    MediawayEncodePathClass::None,
                    "a supported row must report its path class: {row:?}"
                ),
                _ => assert_eq!(
                    row.path_class,
                    MediawayEncodePathClass::None,
                    "path_class is meaningful only when Supported: {row:?}"
                ),
            }
        }
    }
}

/// Windows has a real probe; the answer must agree with actually opening the encoder.
#[cfg(windows)]
#[test]
fn encoder_probe_agrees_with_opening_an_encoder() {
    use mediaway_ffi::pipeline::{
        mediaway_auto_encoder_close, mediaway_auto_encoder_open,
        mediaway_auto_video_encode_config_new,
    };
    let rows = encoder_rows(MediawayPipelineCodecKind::H264, 1280, 720);
    assert!(
        !rows.is_empty(),
        "Windows reports at least one backend row for H.264"
    );
    let any_supported = rows
        .iter()
        .any(|r| r.state == MediawaySupportState::Supported);
    eprintln!("H.264 1280x720 rows: {rows:?}");

    let config = mediaway_auto_video_encode_config_new(
        MediawayPipelineCodecKind::H264,
        1280,
        720,
        mediaway_ffi::pipeline::MediawayRational { num: 1, den: 30 },
    );
    let mut encoder = std::ptr::null_mut();
    let status = unsafe { mediaway_auto_encoder_open(&raw const config, &raw mut encoder) };
    if status == MediawayPipelineStatus::Ok {
        unsafe { mediaway_auto_encoder_close(encoder) };
    }
    assert_eq!(
        any_supported,
        status == MediawayPipelineStatus::Ok,
        "probe said supported={any_supported} but opening an H.264 encoder returned {status:?}"
    );
}

#[test]
fn decoder_probe_rules() {
    let mut state = MediawaySupportState::Unknown;
    assert_eq!(
        unsafe { mediaway_decoder_support(MediawayPipelineCodecKind::H264, std::ptr::null_mut()) },
        MediawayPipelineStatus::InvalidArgument
    );
    let status =
        unsafe { mediaway_decoder_support(MediawayPipelineCodecKind::H264, &raw mut state) };
    assert_eq!(status, MediawayPipelineStatus::Ok);
    assert_ne!(state, MediawaySupportState::Unknown);
}

/// AAC decode exists on Windows and Apple only; the probe must say so before a session is opened.
#[cfg(windows)]
#[test]
fn aac_decode_probe_says_supported_and_a_session_then_opens() {
    use mediaway_ffi::pipeline::{
        mediaway_audio_decode_config_aac, mediaway_audio_decode_session_close,
        mediaway_audio_decode_session_open,
    };
    let mut state = MediawaySupportState::Unknown;
    let status =
        unsafe { mediaway_decoder_support(MediawayPipelineCodecKind::Aac, &raw mut state) };
    assert_eq!(status, MediawayPipelineStatus::Ok);
    assert_eq!(state, MediawaySupportState::Supported);

    let asc = [0x11u8, 0x90];
    let config = mediaway_audio_decode_config_aac(
        48_000,
        2,
        mediaway_ffi::pipeline::MediawayRational {
            num: 1,
            den: 48_000,
        },
        asc.as_ptr(),
        asc.len(),
    );
    let mut session = std::ptr::null_mut();
    let status = unsafe { mediaway_audio_decode_session_open(&raw const config, &raw mut session) };
    assert_eq!(status, MediawayPipelineStatus::Ok);
    unsafe { mediaway_audio_decode_session_close(session) };
}
