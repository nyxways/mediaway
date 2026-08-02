//! Tier 3 — ISOBMFF structural conformance for Mediaway fMP4 mux/demux.
//!
//! Asserts Mediaway crib rules from `docs/spec/iso_14496_12_isobmff.md`
//! (not a paste of the ISO text). Always runs; no `FFmpeg` required.

#![forbid(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "conformance tests may unwrap"
)]

mod common;

use common::{demux_all, mux_tiny_h264_fmp4, walk_children, walk_top_boxes};
use iso_bmff::isobmff::FourCc;

#[test]
fn fmp4_starts_with_ftyp_then_moov_with_mvex() {
    let bytes = mux_tiny_h264_fmp4();
    let tops = walk_top_boxes(&bytes);
    assert!(!tops.is_empty(), "expected at least one top-level box");
    assert_eq!(tops[0].typ, FourCc(*b"ftyp"), "ISO BMFF: ftyp should lead");

    let moov = tops
        .iter()
        .find(|b| b.typ == FourCc(*b"moov"))
        .expect("fMP4 requires moov");
    let kids = walk_children(&bytes, *moov);
    assert!(
        kids.iter().any(|b| b.typ == FourCc(*b"mvex")),
        "fragmented MP4 moov must contain mvex (Mediaway crib)"
    );
    assert!(
        kids.iter().any(|b| b.typ == FourCc(*b"trak")),
        "moov must contain trak"
    );
}

#[test]
fn fmp4_emits_moof_mdat_pairs_with_consistent_sizes() {
    let bytes = mux_tiny_h264_fmp4();
    let tops = walk_top_boxes(&bytes);
    for b in &tops {
        assert!(b.size >= b.header_len);
        assert!(b.offset + b.size <= bytes.len());
        // size==1 largesize path not used by Stage-1 mux; size==0 forbidden in our emitter
        assert_ne!(b.size, 0);
    }

    let mut i = 0;
    while i < tops.len() {
        if tops[i].typ == FourCc(*b"moof") {
            assert!(
                i + 1 < tops.len() && tops[i + 1].typ == FourCc(*b"mdat"),
                "each moof should be followed by mdat in Mediaway fMP4"
            );
            i += 2;
            continue;
        }
        i += 1;
    }
    assert!(
        tops.iter().any(|b| b.typ == FourCc(*b"moof")),
        "expected at least one fragment"
    );
}

#[test]
fn fmp4_demux_recovers_h264_track_and_samples() {
    let bytes = mux_tiny_h264_fmp4();
    let (streams, packets) = demux_all(&bytes);
    assert_eq!(streams, 1);
    assert!(packets >= 1, "expected demuxed samples, got {packets}");
}

#[test]
fn box_headers_are_big_endian_sized() {
    let bytes = mux_tiny_h264_fmp4();
    let tops = walk_top_boxes(&bytes);
    let ftyp = &tops[0];
    let declared = u32::from_be_bytes(bytes[ftyp.offset..ftyp.offset + 4].try_into().unwrap());
    assert_eq!(declared as usize, ftyp.size);
    assert_eq!(&bytes[ftyp.offset + 4..ftyp.offset + 8], b"ftyp");
}
