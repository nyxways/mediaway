#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

fn sample_report() -> ProbeReport {
    ProbeReport {
        format: FormatSummary {
            format_name: "mp4",
            major_brand: Some("isom".to_owned()),
            duration_seconds: Some(1.5),
            stream_count: 1,
        },
        streams: vec![StreamSummary {
            index: 0,
            codec: CodecKind::H264,
            width: Some(1920),
            height: Some(1080),
            time_base: Rational::new(1, 1000),
            packet_count: 45,
            duration_seconds: Some(1.5),
        }],
    }
}

#[test]
fn text_render_includes_both_sections_when_requested() {
    let report = sample_report();
    let text = render_text(&report, true, true);
    assert!(text.contains("[STREAM]"));
    assert!(text.contains("codec_name=h264"));
    assert!(text.contains("width=1920"));
    assert!(text.contains("[FORMAT]"));
    assert!(text.contains("format_name=mp4"));
    assert!(text.contains("major_brand=isom"));
}

#[test]
fn text_render_omits_streams_section_when_not_requested() {
    let report = sample_report();
    let text = render_text(&report, true, false);
    assert!(!text.contains("[STREAM]"));
    assert!(text.contains("[FORMAT]"));
}

#[test]
fn text_render_omits_format_section_when_not_requested() {
    let report = sample_report();
    let text = render_text(&report, false, true);
    assert!(text.contains("[STREAM]"));
    assert!(!text.contains("[FORMAT]"));
}

#[test]
fn text_render_shows_n_a_for_missing_geometry() {
    let mut report = sample_report();
    report.streams[0].width = None;
    report.streams[0].height = None;
    let text = render_text(&report, false, true);
    assert!(text.contains("width=N/A"));
    assert!(text.contains("height=N/A"));
}

#[test]
fn json_render_is_valid_looking_and_contains_fields() {
    let report = sample_report();
    let json = render_json(&report, true, true);
    assert!(json.contains("\"streams\""));
    assert!(json.contains("\"format\""));
    assert!(json.contains("\"codec_name\": \"h264\""));
    assert!(json.contains("\"format_name\": \"mp4\""));
    assert!(json.contains("\"major_brand\": \"isom\""));
}

#[test]
fn json_render_uses_null_for_missing_optional_fields() {
    let mut report = sample_report();
    report.streams[0].width = None;
    report.format.major_brand = None;
    let json = render_json(&report, true, true);
    assert!(json.contains("\"width\": null"));
    assert!(json.contains("\"major_brand\": null"));
}

#[test]
fn json_render_omits_requested_off_sections() {
    let report = sample_report();
    let json = render_json(&report, false, true);
    assert!(json.contains("\"streams\""));
    assert!(!json.contains("\"format\""));
}
