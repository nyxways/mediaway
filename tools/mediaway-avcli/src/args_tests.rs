#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn parses_synthetic_mode_with_default_geometry() {
    let parsed = parse_args(&args(&["--synthetic", "10", "out.mp4"])).unwrap();
    assert_eq!(parsed.mode, CliMode::Synthetic { count: 10 });
    assert_eq!(parsed.geometry, Geometry::default());
    assert_eq!(parsed.output, OutputTarget::File(PathBuf::from("out.mp4")));
}

#[test]
fn parses_from_input_file_and_geometry_override() {
    let parsed = parse_args(&args(&["-i", "in.h264", "-s", "640x480", "out.mp4"])).unwrap();
    assert_eq!(
        parsed.mode,
        CliMode::FromInput {
            input: InputSource::File(PathBuf::from("in.h264"))
        }
    );
    assert_eq!(
        parsed.geometry,
        Geometry {
            width: 640,
            height: 480
        }
    );
}

#[test]
fn dash_input_means_stdin() {
    let parsed = parse_args(&args(&["-i", "-", "out.mp4"])).unwrap();
    assert_eq!(
        parsed.mode,
        CliMode::FromInput {
            input: InputSource::Stdin
        }
    );
}

#[test]
fn dash_output_means_stdout() {
    let parsed = parse_args(&args(&["--synthetic", "1", "-"])).unwrap();
    assert_eq!(parsed.output, OutputTarget::Stdout);
}

#[test]
fn y_flag_is_accepted_as_a_no_op() {
    let parsed = parse_args(&args(&["-y", "--synthetic", "1", "out.mp4"])).unwrap();
    assert_eq!(parsed.mode, CliMode::Synthetic { count: 1 });
}

#[test]
fn missing_output_is_a_usage_error_with_exit_code_two() {
    let err = parse_args(&args(&["--synthetic", "1"])).unwrap_err();
    assert_eq!(err.exit_code(), 2);
}

#[test]
fn missing_input_is_a_usage_error() {
    let err = parse_args(&args(&["out.mp4"])).unwrap_err();
    assert_eq!(err.exit_code(), 2);
}

#[test]
fn combining_i_and_synthetic_is_a_usage_error() {
    let err = parse_args(&args(&["-i", "in.h264", "--synthetic", "1", "out.mp4"])).unwrap_err();
    assert_eq!(err.exit_code(), 2);
}

#[test]
fn unknown_flag_is_a_usage_error() {
    let err = parse_args(&args(&["-bogus", "out.mp4"])).unwrap_err();
    assert_eq!(err.exit_code(), 2);
    assert!(err.to_string().contains("-bogus"));
}

#[test]
fn bad_geometry_is_a_usage_error() {
    let err = parse_args(&args(&["--synthetic", "1", "-s", "notasize", "out.mp4"])).unwrap_err();
    assert_eq!(err.exit_code(), 2);
}

#[test]
fn zero_geometry_is_a_usage_error() {
    let err = parse_args(&args(&["--synthetic", "1", "-s", "0x0", "out.mp4"])).unwrap_err();
    assert_eq!(err.exit_code(), 2);
}

#[test]
fn non_numeric_synthetic_count_is_a_usage_error() {
    let err = parse_args(&args(&["--synthetic", "abc", "out.mp4"])).unwrap_err();
    assert_eq!(err.exit_code(), 2);
}

#[test]
fn extra_positional_argument_is_a_usage_error() {
    let err = parse_args(&args(&["--synthetic", "1", "a.mp4", "b.mp4"])).unwrap_err();
    assert_eq!(err.exit_code(), 2);
}
