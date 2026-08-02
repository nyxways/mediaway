//! Unit tests for edit-list parse / expand.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "unit tests"
)]

use super::{EditListEntry, expand_samples_by_edit_list, parse_elst};
use crate::isobmff::StblSample;

fn sample(dts: i64, dur: u32, cto: i32, key: bool) -> StblSample {
    StblSample {
        offset: dts.max(0) as u64,
        size: 1,
        dts,
        duration: dur,
        cto,
        key,
        discard: false,
    }
}

#[test]
fn parse_elst_v0_three_entries() {
    let mut body = vec![0u8, 0, 0, 0, 0, 0, 0, 3];
    for (dur, mt) in [(500u32, 1024i32), (500, 7168), (1000, 13312)] {
        body.extend_from_slice(&dur.to_be_bytes());
        body.extend_from_slice(&mt.to_be_bytes());
        body.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    }
    let e = parse_elst(&body);
    assert_eq!(e.len(), 3);
    assert_eq!(e[0].segment_duration, 500);
    assert_eq!(e[0].media_time, 1024);
    assert_eq!(e[2].media_time, 13312);
}

#[test]
fn expand_duplicates_across_edits() {
    let raw = vec![
        sample(0, 1000, 0, true),
        sample(1000, 1000, 0, false),
        sample(2000, 1000, 0, true),
        sample(3000, 1000, 0, false),
    ];
    let edits = [
        EditListEntry {
            segment_duration: 2500,
            media_time: 0,
            media_rate: 0x1_0000,
        },
        EditListEntry {
            segment_duration: 2500,
            media_time: 1000,
            media_rate: 0x1_0000,
        },
    ];
    let out = expand_samples_by_edit_list(&raw, &edits, 1000, 1000);
    assert!(
        out.len() > raw.len(),
        "expected duplication, got {}",
        out.len()
    );
}

#[test]
fn clamp_overstated_entry_count() {
    let mut body = vec![0u8, 0, 0, 0, 0, 0, 0, 99];
    body.extend_from_slice(&1u32.to_be_bytes());
    body.extend_from_slice(&0i32.to_be_bytes());
    body.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    let e = parse_elst(&body);
    assert_eq!(e.len(), 1);
}

#[test]
fn expand_remaps_pts_and_marks_discard() {
    // media_time=1000 lands mid-GOP → walk back to key at 0; that sample is discard.
    let raw = vec![
        sample(0, 500, 0, true),
        sample(500, 500, 0, false),
        sample(1000, 500, 0, false),
        sample(1500, 500, 0, false),
    ];
    let edits = [EditListEntry {
        segment_duration: 1000,
        media_time: 1000,
        media_rate: 0x1_0000,
    }];
    let out = expand_samples_by_edit_list(&raw, &edits, 1000, 1000);
    assert!(out.len() >= 3, "got {}", out.len());
    assert!(out[0].discard);
    assert_eq!(out[0].dts, -1000);
    let first_keep = out.iter().find(|s| !s.discard).expect("in-range");
    assert_eq!(first_keep.dts, 0);
    assert_eq!(sample_cts_pub(first_keep), 0);
}

fn sample_cts_pub(s: &StblSample) -> i64 {
    s.dts.saturating_add(i64::from(s.cto))
}

#[test]
fn fate_mov_neg_first_pts_discard() {
    use crate::Demuxer;
    use std::path::PathBuf;

    let root = std::env::var("MEDIAWAY_FATE_SAMPLES")
        .or_else(|_| std::env::var("FATE_SAMPLES"))
        .ok();
    let Some(root) = root else {
        eprintln!("skip: MEDIAWAY_FATE_SAMPLES unset");
        return;
    };
    let path = PathBuf::from(root).join("mov/mov_neg_first_pts_discard.mov");
    if !path.is_file() {
        eprintln!("skip: missing {}", path.display());
        return;
    }
    let bytes = std::fs::read(&path).expect("read");
    let mut d = Demuxer::new();
    d.push_bytes(&bytes);
    let mut packets = Vec::new();
    while let Some(p) = d.poll_packet() {
        packets.push(p);
    }
    assert_eq!(packets.len(), 30, "packet count");
    assert_eq!(packets[0].pts, -7680);
    assert_eq!(packets[0].dts, -8704);
    assert!(packets[0].is_discard);
    assert!(packets[0].is_keyframe);
    let first_keep = packets.iter().find(|p| !p.is_discard).expect("non-discard");
    assert_eq!(first_keep.pts, 0);
    assert!(
        packets.iter().filter(|p| !p.is_discard).all(|p| p.pts >= 0),
        "non-discard PTS must be non-negative (stream start_time 0)"
    );
}

#[test]
fn fate_mov_3elist_expands_to_ffprobe_packets() {
    use crate::Demuxer;
    use std::path::PathBuf;

    let root = std::env::var("MEDIAWAY_FATE_SAMPLES")
        .or_else(|_| std::env::var("FATE_SAMPLES"))
        .ok();
    let Some(root) = root else {
        eprintln!("skip: MEDIAWAY_FATE_SAMPLES unset");
        return;
    };
    let path = PathBuf::from(root).join("mov/mov-3elist.mov");
    if !path.is_file() {
        eprintln!("skip: missing {}", path.display());
        return;
    }
    let bytes = std::fs::read(&path).expect("read");
    let mut d = Demuxer::new();
    d.push_bytes(&bytes);
    let mut n = 0usize;
    while d.poll_packet().is_some() {
        n += 1;
    }
    assert_eq!(n, 74, "mov-3elist demux packets");
}
