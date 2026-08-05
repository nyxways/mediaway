#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

#[test]
fn std_level_idc_maps_every_defined_level() {
    let cases = [
        (30, native::STD_VIDEO_H265_LEVEL_IDC_1_0),
        (60, native::STD_VIDEO_H265_LEVEL_IDC_2_0),
        (63, native::STD_VIDEO_H265_LEVEL_IDC_2_1),
        (90, native::STD_VIDEO_H265_LEVEL_IDC_3_0),
        (93, native::STD_VIDEO_H265_LEVEL_IDC_3_1),
        (120, native::STD_VIDEO_H265_LEVEL_IDC_4_0),
        (186, native::STD_VIDEO_H265_LEVEL_IDC_6_2),
    ];
    for (raw, expected) in cases {
        assert_eq!(std_level_idc(raw), expected, "raw general_level_idc {raw}");
    }
    // An unrecognized in-between raw value rounds up to the next known level
    // rather than silently under-reporting what was actually signaled.
    assert_eq!(std_level_idc(91), native::STD_VIDEO_H265_LEVEL_IDC_3_1);
}
