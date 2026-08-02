//! Unit tests for basic-header encode/decode and the 24/32-bit helpers.

#![cfg(test)]
#![allow(clippy::unwrap_used, reason = "unit tests")]

use super::{
    read_basic_header, read_u24_be, read_u32_be, read_u32_le, write_basic_header, write_u24_be,
};

#[test]
fn one_byte_form_round_trips_for_small_csid() {
    let mut out = Vec::new();
    write_basic_header(&mut out, 2, 5);
    assert_eq!(out, vec![(2 << 6) | 5]);
    let parsed = read_basic_header(&out).unwrap();
    assert_eq!((parsed.fmt, parsed.csid, parsed.len), (2, 5, 1));
}

#[test]
fn two_byte_form_round_trips_for_mid_csid() {
    let mut out = Vec::new();
    write_basic_header(&mut out, 0, 64);
    assert_eq!(out, vec![0x00, 0x00]); // low6=0, second byte = csid-64 = 0
    let parsed = read_basic_header(&out).unwrap();
    assert_eq!((parsed.fmt, parsed.csid, parsed.len), (0, 64, 2));

    let mut out2 = Vec::new();
    write_basic_header(&mut out2, 3, 319);
    let parsed2 = read_basic_header(&out2).unwrap();
    assert_eq!((parsed2.fmt, parsed2.csid, parsed2.len), (3, 319, 2));
}

#[test]
fn three_byte_form_round_trips_for_large_csid() {
    let mut out = Vec::new();
    write_basic_header(&mut out, 1, 65599);
    assert_eq!(out.len(), 3);
    let parsed = read_basic_header(&out).unwrap();
    assert_eq!((parsed.fmt, parsed.csid, parsed.len), (1, 65599, 3));
}

#[test]
fn read_basic_header_none_when_incomplete() {
    assert!(read_basic_header(&[]).is_none());
    // low6 == 0 needs a second byte.
    assert!(read_basic_header(&[0x00]).is_none());
    // low6 == 1 needs two more bytes.
    assert!(read_basic_header(&[0x01, 0x00]).is_none());
}

#[test]
fn u24_be_round_trips() {
    let mut out = Vec::new();
    write_u24_be(&mut out, 0x0102_0304 & 0x00FF_FFFF);
    assert_eq!(out, vec![0x02, 0x03, 0x04]);
    assert_eq!(read_u24_be(&out), 0x0002_0304);
}

#[test]
fn u32_be_and_le_read_differ_in_byte_order() {
    let bytes = [0x01, 0x02, 0x03, 0x04];
    assert_eq!(read_u32_be(&bytes), 0x0102_0304);
    assert_eq!(read_u32_le(&bytes), 0x0403_0201);
}
