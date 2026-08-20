//! Live muxer timing tests — sample durations derive from dts deltas
//! between consecutive samples.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "unit tests"
)]

use crate::isobmff::{FourCc, parse_header, parse_moof};
use crate::mux::Muxer;
use crate::types::{Bytes, Codec, Rational, Sample, Track};

fn track(id: u32) -> Track {
    Track {
        id,
        codec: Codec::H264,
        time_base: Rational::new(1, 30),
        width: 16,
        height: 16,
        extra_data: Bytes::new(),
    }
}

fn sample(stream_id: u32, dts: i64, duration: u64, key: bool) -> Sample {
    Sample {
        stream_id,
        pts: dts,
        dts,
        duration,
        is_keyframe: key,
        is_discard: false,
        // not Annex-B, so `to_avcc` passes the payload through untouched.
        payload: Bytes::from_static(&[1, 2, 3]),
    }
}

fn mux(samples: &[Sample]) -> Vec<u8> {
    let mut m = Muxer::new();
    m.add_track(track(0)).unwrap();
    let mut live = m.begin();
    for s in samples {
        live.push_packet(s).unwrap();
    }
    live.flush();
    let mut out = Vec::new();
    live.poll_bytes(&mut out);
    out
}

/// `(base_dts, [(duration, key)])` per `moof` in the muxed output.
fn moofs(out: &[u8]) -> Vec<(u64, Vec<(u32, bool)>)> {
    let mut res = Vec::new();
    let mut pos = 0usize;
    while pos + 8 <= out.len() {
        let Some(hdr) = parse_header(&out[pos..]) else {
            break;
        };
        if hdr.typ == FourCc(*b"moof") {
            let info = parse_moof(&out[pos + hdr.header_len..pos + hdr.size]);
            res.push((
                info.base_dts,
                info.samples.iter().map(|s| (s.duration, s.key)).collect(),
            ));
        }
        pos += hdr.size;
    }
    res
}

fn durations(frag: &(u64, Vec<(u32, bool)>)) -> Vec<u32> {
    frag.1.iter().map(|(d, _)| *d).collect()
}

#[test]
fn durations_derive_from_dts_deltas() {
    // Caller supplies no durations; dts 0/30/60 at timescale 30 (30 fps).
    let samples = [
        sample(0, 0, 0, true),
        sample(0, 30, 0, false),
        sample(0, 60, 0, false),
    ];
    let frags = moofs(&mux(&samples));
    assert_eq!(frags.len(), 1);
    assert_eq!(frags[0].0, 0, "fragment base dts");
    // deltas [30, 30]; the last sample is estimated from the previous delta.
    assert_eq!(durations(&frags[0]), vec![30, 30, 30]);
}

#[test]
fn caller_duration_trusted_for_last_sample_only() {
    // Non-last samples always take dts deltas; the last sample keeps the
    // caller-provided duration.
    let samples = [
        sample(0, 0, 999, true),
        sample(0, 30, 999, false),
        sample(0, 60, 45, false),
    ];
    let frags = moofs(&mux(&samples));
    assert_eq!(durations(&frags[0]), vec![30, 30, 45]);
}

#[test]
fn lone_sample_fragment_defaults_to_one_tick() {
    let samples = [sample(0, 42, 0, true)];
    let frags = moofs(&mux(&samples));
    assert_eq!(durations(&frags[0]), vec![1]);
}

#[test]
fn non_monotonic_dts_clamps_to_one_tick() {
    // Out-of-order dts (e.g. a B-frame encoder feeding display order) must
    // degrade to a 1-tick duration — never a zero or u32::MAX sample that
    // would collapse or explode the demuxed timeline.
    let samples = [
        sample(0, 0, 0, true),
        sample(0, 30, 0, false),
        sample(0, 10, 0, false), // dts goes backwards
        sample(0, 40, 0, false),
    ];
    let frags = moofs(&mux(&samples));
    assert_eq!(durations(&frags[0]), vec![30, 1, 30, 30]);
}

#[test]
fn set_track_extra_data_backfills_before_header_written() {
    // A real avcC — distinct from write_avc1's placeholder, so its presence in the
    // output is unambiguous.
    let real_avcc = Bytes::from_static(&[1, 0x64, 0, 0x1f, 0xff, 0xe1, 0, 4, 9, 9, 9, 9, 1, 0, 0]);
    let mut m = Muxer::new();
    m.add_track(track(0)).unwrap();
    let mut live = m.begin();

    live.set_track_extra_data(0, real_avcc.clone());
    assert_eq!(live.tracks()[0].extra_data, real_avcc);

    live.push_packet(&sample(0, 0, 30, true)).unwrap();
    live.flush();
    let mut out = Vec::new();
    live.poll_bytes(&mut out);

    let mut needle = Vec::new();
    needle.extend_from_slice(&real_avcc);
    assert!(
        out.windows(needle.len()).any(|w| w == needle.as_slice()),
        "expected the backfilled avcC bytes in the muxed output, not the placeholder"
    );
}

#[test]
fn set_track_extra_data_never_overwrites_a_real_value() {
    let first = Bytes::from_static(&[1, 0x64, 0, 0x1f, 0xff, 0xe1, 0, 1, 0xaa, 1, 0, 0]);
    let second = Bytes::from_static(&[1, 0x64, 0, 0x1f, 0xff, 0xe1, 0, 1, 0xbb, 1, 0, 0]);
    let mut m = Muxer::new();
    m.add_track(track(0)).unwrap();
    let mut live = m.begin();

    live.set_track_extra_data(0, first.clone());
    live.set_track_extra_data(0, second);
    assert_eq!(live.tracks()[0].extra_data, first, "first write wins");
}

#[test]
fn set_track_extra_data_is_a_noop_after_header_written() {
    let real_avcc = Bytes::from_static(&[1, 0x64, 0, 0x1f, 0xff, 0xe1, 0, 1, 0xcc, 1, 0, 0]);
    let mut m = Muxer::new();
    m.add_track(track(0)).unwrap();
    let mut live = m.begin();

    // First packet, with an empty extra_data track, writes the moov header using
    // write_avc1's placeholder (no in-band Annex-B SPS/PPS in `sample`'s payload).
    live.push_packet(&sample(0, 0, 30, true)).unwrap();
    live.set_track_extra_data(0, real_avcc);
    assert!(
        live.tracks()[0].extra_data.is_empty(),
        "too late to matter — the moov header is already written"
    );
}

#[test]
fn fragment_bases_advance_with_dts() {
    // Keyframes force fragment flushes; each fragment's tfdt is its first
    // sample's dts, and every duration stays 30 ticks.
    let samples: Vec<Sample> = (0..60).map(|i| sample(0, i * 30, 0, i % 30 == 0)).collect();
    let frags = moofs(&mux(&samples));
    assert_eq!(frags.len(), 2);
    assert_eq!(
        frags.iter().map(|f| f.0).collect::<Vec<_>>(),
        vec![0, 900],
        "fragment bases"
    );
    for frag in &frags {
        assert!(durations(frag).iter().all(|&d| d == 30));
    }
}
