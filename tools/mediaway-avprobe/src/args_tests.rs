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
fn parses_bare_input_and_defaults_to_showing_both_sections() {
    let parsed = parse_args(&args(&["movie.mp4"])).unwrap();
    assert_eq!(parsed.input, PathBuf::from("movie.mp4"));
    assert!(parsed.show_format);
    assert!(parsed.show_streams);
    assert_eq!(parsed.output_format, OutputFormat::Default);
}

#[test]
fn show_format_only_does_not_imply_show_streams() {
    let parsed = parse_args(&args(&["-show_format", "movie.mp4"])).unwrap();
    assert!(parsed.show_format);
    assert!(!parsed.show_streams);
}

#[test]
fn parses_of_json() {
    let parsed = parse_args(&args(&["-of", "json", "-show_streams", "movie.mp4"])).unwrap();
    assert_eq!(parsed.output_format, OutputFormat::Json);
    assert!(parsed.show_streams);
    assert!(!parsed.show_format);
}

#[test]
fn accepts_i_flag_for_input() {
    let parsed = parse_args(&args(&["-i", "movie.mp4"])).unwrap();
    assert_eq!(parsed.input, PathBuf::from("movie.mp4"));
}

#[test]
fn missing_input_is_a_usage_error_with_exit_code_two() {
    let err = parse_args(&args(&["-show_format"])).unwrap_err();
    assert_eq!(err.exit_code(), 2);
}

#[test]
fn unknown_flag_is_a_usage_error() {
    let err = parse_args(&args(&["-bogus", "movie.mp4"])).unwrap_err();
    assert_eq!(err.exit_code(), 2);
    assert!(err.to_string().contains("-bogus"));
}

#[test]
fn of_without_value_is_a_usage_error() {
    let err = parse_args(&args(&["-of"])).unwrap_err();
    assert_eq!(err.exit_code(), 2);
}

#[test]
fn unsupported_of_value_is_a_usage_error() {
    let err = parse_args(&args(&["-of", "xml", "movie.mp4"])).unwrap_err();
    assert_eq!(err.exit_code(), 2);
}

#[test]
fn extra_positional_argument_is_a_usage_error() {
    let err = parse_args(&args(&["a.mp4", "b.mp4"])).unwrap_err();
    assert_eq!(err.exit_code(), 2);
}
