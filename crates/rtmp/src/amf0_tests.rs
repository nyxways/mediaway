//! Unit tests for AMF0 encode — byte-shape checks against hand-computed expected sequences
//! for each of the 6 supported type markers.

#![cfg(test)]
#![allow(clippy::unwrap_used, reason = "unit tests")]

use super::{
    write_boolean, write_ecma_array_start, write_null, write_number, write_object_end,
    write_object_start, write_property_name, write_string,
};
use crate::error::Error;

#[test]
fn number_is_marker_plus_be_f64() {
    let mut out = Vec::new();
    write_number(&mut out, 3.0);
    let mut expected = vec![0x00];
    expected.extend_from_slice(&3.0f64.to_be_bytes());
    assert_eq!(out, expected);
    assert_eq!(out.len(), 9);
}

#[test]
fn boolean_true_is_marker_plus_one() {
    let mut out = Vec::new();
    write_boolean(&mut out, true);
    assert_eq!(out, vec![0x01, 0x01]);
}

#[test]
fn boolean_false_is_marker_plus_zero() {
    let mut out = Vec::new();
    write_boolean(&mut out, false);
    assert_eq!(out, vec![0x01, 0x00]);
}

#[test]
fn string_is_marker_plus_u16_len_plus_utf8() {
    let mut out = Vec::new();
    write_string(&mut out, "abc").unwrap();
    assert_eq!(out, vec![0x02, 0x00, 0x03, b'a', b'b', b'c']);
}

#[test]
fn string_too_long_errors() {
    let value = "a".repeat(usize::from(u16::MAX) + 1);
    let mut out = Vec::new();
    let err = write_string(&mut out, &value).unwrap_err();
    assert!(matches!(err, Error::StringTooLong(n) if n == value.len()));
    assert!(out.is_empty());
}

#[test]
fn null_is_bare_marker() {
    let mut out = Vec::new();
    write_null(&mut out);
    assert_eq!(out, vec![0x05]);
}

#[test]
fn object_round_shape_with_one_property_and_terminator() {
    let mut out = Vec::new();
    write_object_start(&mut out);
    write_property_name(&mut out, "a").unwrap();
    write_number(&mut out, 1.0);
    write_object_end(&mut out);

    let mut expected = vec![0x03]; // Object marker
    expected.extend_from_slice(&[0x00, 0x01, b'a']); // property name "a"
    expected.push(0x00); // Number marker
    expected.extend_from_slice(&1.0f64.to_be_bytes());
    expected.extend_from_slice(&[0x00, 0x00, 0x09]); // terminator
    assert_eq!(out, expected);
}

#[test]
fn ecma_array_shape_with_count_property_and_terminator() {
    let mut out = Vec::new();
    write_ecma_array_start(&mut out, 1);
    write_property_name(&mut out, "width").unwrap();
    write_number(&mut out, 1920.0);
    write_object_end(&mut out);

    let mut expected = vec![0x08]; // ECMA Array marker
    expected.extend_from_slice(&1u32.to_be_bytes()); // associative-array-count
    expected.extend_from_slice(&[0x00, 0x05]); // "width" length
    expected.extend_from_slice(b"width");
    expected.push(0x00); // Number marker
    expected.extend_from_slice(&1920.0f64.to_be_bytes());
    expected.extend_from_slice(&[0x00, 0x00, 0x09]); // terminator
    assert_eq!(out, expected);
}

#[test]
fn property_name_too_long_errors() {
    let key = "k".repeat(usize::from(u16::MAX) + 1);
    let mut out = Vec::new();
    let err = write_property_name(&mut out, &key).unwrap_err();
    assert!(matches!(err, Error::StringTooLong(n) if n == key.len()));
}
