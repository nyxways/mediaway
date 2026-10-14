#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;
use crate::bitstream::avc::annex_b_replacing_sps;

/// libx264 High profile, 640 x 400, cropped by `FFmpeg`'s `h264_metadata` to 480 x 360 at (80, 20).
const CROPPED_AT_OFFSET: [u8; 28] = [
    0x67, 0x64, 0x00, 0x1e, 0xac, 0xb2, 0x01, 0x40, 0x67, 0x82, 0x90, 0x52, 0x2c, 0x5e, 0x02, 0x20,
    0x00, 0x00, 0x03, 0x00, 0x20, 0x00, 0x00, 0x07, 0x81, 0xe2, 0xc5, 0xc9,
];

/// The same encode cropped to 480 x 360 from the right and bottom only.
const CROPPED_RIGHT_BOTTOM: [u8; 28] = [
    0x67, 0x64, 0x00, 0x1e, 0xac, 0xb2, 0x01, 0x40, 0x67, 0xc0, 0xa3, 0x0a, 0xe0, 0x22, 0x00, 0x00,
    0x03, 0x00, 0x02, 0x00, 0x00, 0x03, 0x00, 0x78, 0x1e, 0x2c, 0x5c, 0x90,
];

/// The same encode without a crop.
const UNCROPPED: [u8; 25] = [
    0x67, 0x64, 0x00, 0x1e, 0xac, 0xb2, 0x01, 0x40, 0x67, 0x60, 0x22, 0x00, 0x00, 0x03, 0x00, 0x02,
    0x00, 0x00, 0x03, 0x00, 0x78, 0x1e, 0x2c, 0x5c, 0x90,
];

const OFFSET_CROP: SpsCrop = SpsCrop {
    left: 80,
    right: 80,
    top: 20,
    bottom: 20,
};

#[test]
fn reads_the_coded_size_and_the_crop() {
    let geometry = parse_sps_geometry(&CROPPED_AT_OFFSET).unwrap();
    assert_eq!((geometry.coded_width, geometry.coded_height), (640, 400));
    assert_eq!(geometry.crop, OFFSET_CROP);
    assert_eq!(geometry.crop_unit, (2, 2));
    assert!(geometry.crop.has_offset());

    let right_bottom = parse_sps_geometry(&CROPPED_RIGHT_BOTTOM).unwrap().crop;
    assert_eq!(
        right_bottom,
        SpsCrop {
            left: 0,
            right: 160,
            top: 0,
            bottom: 40
        }
    );
    assert!(!right_bottom.has_offset());
    assert_eq!(
        parse_sps_geometry(&UNCROPPED).unwrap().crop,
        SpsCrop::default()
    );
}

#[test]
fn clearing_the_crop_gives_the_uncropped_sps_and_putting_it_back_the_original() {
    let cleared = sps_with_crop(&CROPPED_AT_OFFSET, SpsCrop::default()).unwrap();
    assert_eq!(cleared, UNCROPPED, "every field after the crop kept");
    assert_eq!(
        sps_with_crop(&cleared, OFFSET_CROP).unwrap(),
        CROPPED_AT_OFFSET
    );
}

#[test]
fn a_crop_off_its_step_or_showing_nothing_is_refused() {
    let odd = SpsCrop {
        left: 1,
        ..SpsCrop::default()
    };
    assert!(sps_with_crop(&UNCROPPED, odd).is_none());
    let everything = SpsCrop {
        left: 320,
        right: 320,
        ..SpsCrop::default()
    };
    assert!(sps_with_crop(&UNCROPPED, everything).is_none());
    // A PPS is not an SPS.
    assert!(parse_sps_geometry(&[0x68, 0xeb, 0xe3, 0xcb]).is_none());
}

fn annex_b(nals: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for nal in nals {
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(nal);
    }
    out
}

#[test]
fn an_access_units_sps_is_replaced_and_the_rest_kept() {
    let aud: &[u8] = &[0x09, 0xf0];
    let pps: &[u8] = &[0x68, 0xeb, 0xe3, 0xcb, 0x22, 0xc0];
    let slice: &[u8] = &[0x65, 0x88, 0x84, 0x00, 0x33, 0xff];
    // A 3-byte start code before the PPS is kept as it was.
    let mut data = annex_b(&[aud, &CROPPED_AT_OFFSET]);
    data.extend_from_slice(&[0, 0, 1]);
    data.extend_from_slice(pps);
    data.extend_from_slice(&annex_b(&[slice]));

    let out = annex_b_replacing_sps(&data, |nal| sps_with_crop(nal, SpsCrop::default())).unwrap();
    let mut want = annex_b(&[aud, &UNCROPPED]);
    want.extend_from_slice(&[0, 0, 1]);
    want.extend_from_slice(pps);
    want.extend_from_slice(&annex_b(&[slice]));
    assert_eq!(&out[..], &want[..]);
}

#[test]
fn nothing_replaced_gives_none_and_slices_are_not_looked_into() {
    let slice_with_sps_bytes: Vec<u8> =
        [&[0x65u8, 0x88][..], &[0, 0, 1], &CROPPED_AT_OFFSET].concat();
    let data = annex_b(&[&slice_with_sps_bytes]);
    let mut seen = 0;
    let out = annex_b_replacing_sps(&data, |_| {
        seen += 1;
        Some(Vec::new())
    });
    assert!(out.is_none());
    assert_eq!(seen, 0, "the walk stops at the first slice");

    let data = annex_b(&[&UNCROPPED, &[0x65, 0x88]]);
    assert!(annex_b_replacing_sps(&data, |_| None).is_none());
}
