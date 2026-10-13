//! A non-fragmented head, read back with the parsers that read other programs' files.

#![cfg(test)]
#![allow(clippy::unwrap_used, reason = "unit tests")]

use super::{ProgressiveTrack, write_indexed_moov, write_progressive};
use crate::isobmff::{EditListEntry, FourCc, StblSample, parse_edts, parse_header, parse_moov};
use crate::types::{Bytes, Codec, Rational, Track};

fn video() -> Track {
    Track {
        id: 0,
        codec: Codec::H264,
        time_base: Rational::new(1, 1_000_000),
        width: 64,
        height: 48,
        extra_data: Bytes::new(),
    }
}

fn audio() -> Track {
    Track {
        id: 1,
        codec: Codec::Aac,
        time_base: Rational::new(1, 1_000_000),
        width: 0,
        height: 0,
        extra_data: Bytes::from_static(&[0x11, 0x90]),
    }
}

fn sample(offset: u64, size: u32, dts: i64, duration: u32, cto: i32, key: bool) -> StblSample {
    StblSample {
        offset,
        size,
        dts,
        duration,
        cto,
        key,
        discard: false,
    }
}

/// The head's boxes: type and body.
fn boxes(head: &[u8]) -> Vec<(FourCc, &[u8])> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 8 <= head.len() {
        let hdr = parse_header(&head[at..]).unwrap();
        // The `mdat`'s payload is not in the head.
        let end = (at + hdr.size).min(head.len());
        out.push((hdr.typ, &head[at + hdr.header_len..end]));
        at += hdr.size;
    }
    out
}

fn moov_body(head: &[u8]) -> &[u8] {
    boxes(head)
        .into_iter()
        .find(|(t, _)| *t == FourCc(*b"moov"))
        .unwrap()
        .1
}

fn moov_of(head: &[u8]) -> Vec<crate::isobmff::MoovTrack> {
    parse_moov(moov_body(head)).into_vec()
}

fn single(track: &Track, samples: &[StblSample], edits: &[EditListEntry], payload: u64) -> Vec<u8> {
    let mut head = Vec::new();
    write_progressive(
        &mut head,
        &[ProgressiveTrack {
            track,
            samples,
            edits,
        }],
        payload,
    );
    head
}

#[test]
fn samples_come_back_as_they_went_in() {
    // Video with B-frame style offsets (one negative: version 1 of `ctts`) and a sound packet
    // between the pictures.
    let v = [
        sample(0, 100, 0, 40_000, 80_000, true),
        sample(300, 20, 40_000, 40_000, -40_000, false),
        sample(500, 30, 80_000, 40_000, 0, false),
    ];
    let a = [
        sample(100, 200, 0, 21_333, 0, true),
        sample(400, 100, 21_333, 21_333, 0, true),
    ];
    let (video, audio) = (video(), audio());
    let mut head = Vec::new();
    write_progressive(
        &mut head,
        &[
            ProgressiveTrack {
                track: &video,
                samples: &v,
                edits: &[],
            },
            ProgressiveTrack {
                track: &audio,
                samples: &a,
                edits: &[],
            },
        ],
        600,
    );
    let tracks = moov_of(&head);
    assert_eq!(tracks.len(), 2);
    let base = head.len() as u64;
    let placed = |list: &[StblSample]| -> Vec<StblSample> {
        list.iter()
            .map(|s| StblSample {
                offset: base + s.offset,
                ..*s
            })
            .collect()
    };
    assert_eq!(tracks[0].samples, placed(&v));
    assert_eq!(tracks[1].samples, placed(&a));
    assert_eq!(tracks[0].info.id, 0);
    assert_eq!(tracks[1].info.id, 1);
}

#[test]
fn the_mdat_header_carries_the_payload_length() {
    let head = single(&video(), &[sample(0, 10, 0, 33_333, 0, true)], &[], 10);
    let (size, kind) = head[head.len() - 8..].split_at(4);
    assert_eq!(kind, b"mdat");
    assert_eq!(u32::from_be_bytes(size.try_into().unwrap()), 18);
}

#[test]
fn a_file_over_four_gigabytes_has_wide_offsets_and_a_wide_mdat() {
    let v = [
        sample(0, 100, 0, 33_333, 0, true),
        sample(4_500_000_000, 100, 33_333, 33_333, 0, false),
    ];
    let head = single(&video(), &v, &[], 4_600_000_000);
    let at = head.len() - 16;
    assert_eq!(u32::from_be_bytes(head[at..at + 4].try_into().unwrap()), 1);
    assert_eq!(&head[at + 4..at + 8], b"mdat");
    assert_eq!(
        u64::from_be_bytes(head[at + 8..].try_into().unwrap()),
        4_600_000_016
    );
    let tracks = moov_of(&head);
    let base = head.len() as u64;
    assert_eq!(tracks[0].samples[0].offset, base);
    assert_eq!(tracks[0].samples[1].offset, base + 4_500_000_000);
}

#[test]
fn an_edit_list_is_written_and_read_by_the_parser_that_applies_it() {
    // Five one-second pictures, the first a keyframe; the edit shows from 2 s, 2.5 s long.
    let v: Vec<StblSample> = (0..5u32)
        .map(|n| {
            sample(
                u64::from(n) * 10,
                10,
                i64::from(n) * 1_000_000,
                1_000_000,
                0,
                n == 0,
            )
        })
        .collect();
    let edit = EditListEntry {
        segment_duration: 2_500,
        media_time: 2_000_000,
        media_rate: 0x0001_0000,
    };
    let head = single(&video(), &v, &[edit], 50);
    // The `elst` is in the `trak`'s `edts`.
    let trak = boxes(moov_body(&head))
        .into_iter()
        .find(|(t, _)| *t == FourCc(*b"trak"))
        .unwrap()
        .1;
    let edts = boxes(trak)
        .into_iter()
        .find(|(t, _)| *t == FourCc(*b"edts"))
        .unwrap()
        .1;
    assert_eq!(parse_edts(edts), vec![edit]);
    // Applied: the picture at 2 s is at zero, the earlier ones before it and set to be discarded.
    let samples = &moov_of(&head)[0].samples;
    assert!(samples.iter().any(|s| s.dts == 0 && !s.discard));
    assert!(samples.iter().any(|s| s.dts < 0 && s.discard));
}

#[test]
fn the_movie_lasts_as_long_as_its_samples_or_its_edit() {
    let v = [
        sample(0, 10, 0, 500_000, 0, true),
        sample(10, 10, 500_000, 500_001, 0, true),
    ];
    let duration_of = |head: &[u8]| {
        let mvhd = boxes(moov_body(head))
            .into_iter()
            .find(|(t, _)| *t == FourCc(*b"mvhd"))
            .unwrap()
            .1;
        // Version and flags, creation, modification, timescale, duration.
        assert_eq!(u32::from_be_bytes(mvhd[12..16].try_into().unwrap()), 1000);
        u32::from_be_bytes(mvhd[16..20].try_into().unwrap())
    };
    // 1,000,001 us is 1001 ms, rounded up.
    assert_eq!(duration_of(&single(&video(), &v, &[], 20)), 1001);
    let edit = EditListEntry {
        segment_duration: 400,
        media_time: 500_000,
        media_rate: 0x0001_0000,
    };
    assert_eq!(duration_of(&single(&video(), &v, &[edit], 20)), 400);
}

#[test]
fn a_track_with_no_samples_has_empty_tables() {
    let head = single(&video(), &[], &[], 0);
    assert!(moov_of(&head)[0].samples.is_empty());
}

#[test]
fn a_track_that_starts_late_has_an_empty_edit_first() {
    let v = [sample(0, 10, 0, 500_000, 0, true)];
    let wait = EditListEntry {
        segment_duration: 30,
        media_time: -1,
        media_rate: 0x0001_0000,
    };
    let play = EditListEntry {
        segment_duration: 500,
        media_time: 0,
        media_rate: 0x0001_0000,
    };
    let head = single(&video(), &v, &[wait, play], 10);
    let trak = boxes(moov_body(&head))
        .into_iter()
        .find(|(t, _)| *t == FourCc(*b"trak"))
        .unwrap()
        .1;
    let edts = boxes(trak)
        .into_iter()
        .find(|(t, _)| *t == FourCc(*b"edts"))
        .unwrap()
        .1;
    assert_eq!(parse_edts(edts), vec![wait, play]);
}

/// Two tracks as a fragmented file lays them out: runs of samples, one run to a fragment, the
/// tracks taking turns, other boxes between. Offsets are absolute.
fn fragments() -> (Vec<StblSample>, Vec<StblSample>) {
    let v = vec![
        sample(1_000, 100, 0, 40_000, 80_000, true),
        sample(1_100, 20, 40_000, 40_000, -40_000, false),
        sample(1_120, 30, 80_000, 40_000, 0, false),
        // The next video fragment: after the sound's, and a record box.
        sample(2_000, 50, 120_000, 40_000, 0, false),
        sample(2_050, 60, 160_000, 40_000, 0, false),
    ];
    let a = vec![
        sample(1_500, 200, 0, 21_333, 0, true),
        sample(1_700, 100, 21_333, 21_333, 0, true),
    ];
    (v, a)
}

#[test]
fn an_indexed_moov_is_only_a_moov_and_points_at_samples_where_they_are() {
    let (v, a) = fragments();
    let (video, audio) = (video(), audio());
    let mut moov = Vec::new();
    write_indexed_moov(
        &mut moov,
        &[
            ProgressiveTrack {
                track: &video,
                samples: &v,
                edits: &[],
            },
            ProgressiveTrack {
                track: &audio,
                samples: &a,
                edits: &[],
            },
        ],
    );
    let top = boxes(&moov);
    assert_eq!(top.len(), 1, "a moov and nothing else");
    assert_eq!(top[0].0, FourCc(*b"moov"));
    let tracks = moov_of(&moov);
    assert_eq!(tracks[0].samples, v);
    assert_eq!(tracks[1].samples, a);
    // No `mvex`: this is the non-fragmented movie.
    assert!(!boxes(top[0].1).iter().any(|(t, _)| *t == FourCc(*b"mvex")));
}

#[test]
fn an_indexed_moov_shares_a_chunk_between_adjacent_samples() {
    let (v, _) = fragments();
    let video = video();
    let mut moov = Vec::new();
    write_indexed_moov(
        &mut moov,
        &[ProgressiveTrack {
            track: &video,
            samples: &v,
            edits: &[],
        }],
    );
    // Two fragments: a chunk of three samples, then one of two.
    let stbl = find_path(&moov, &[b"moov", b"trak", b"mdia", b"minf", b"stbl"]);
    let stsc = find_path(stbl, &[b"stsc"]);
    let entries: Vec<u32> = stsc[8..]
        .chunks(4)
        .map(|c| u32::from_be_bytes(c.try_into().unwrap()))
        .collect();
    // (first chunk, samples per chunk, description) twice.
    assert_eq!(entries, [1, 3, 1, 2, 2, 1]);
    let stco = find_path(stbl, &[b"stco"]);
    assert_eq!(u32::from_be_bytes(stco[4..8].try_into().unwrap()), 2);
    assert_eq!(u32::from_be_bytes(stco[8..12].try_into().unwrap()), 1_000);
    assert_eq!(u32::from_be_bytes(stco[12..16].try_into().unwrap()), 2_000);
}

/// The body of the box at the end of `path`, each a child of the one before (`bytes` is a run of
/// boxes).
fn find_path<'a>(bytes: &'a [u8], path: &[&[u8; 4]]) -> &'a [u8] {
    let mut level = bytes;
    for kind in path {
        level = boxes(level)
            .into_iter()
            .find(|(t, _)| t.0 == **kind)
            .unwrap()
            .1;
    }
    level
}

#[test]
fn an_indexed_moov_past_four_gigabytes_has_wide_offsets() {
    let v = [
        sample(1_000, 100, 0, 33_333, 0, true),
        sample(4_500_000_000, 100, 33_333, 33_333, 0, false),
    ];
    let video = video();
    let mut moov = Vec::new();
    write_indexed_moov(
        &mut moov,
        &[ProgressiveTrack {
            track: &video,
            samples: &v,
            edits: &[],
        }],
    );
    let stbl = find_path(&moov, &[b"moov", b"trak", b"mdia", b"minf", b"stbl"]);
    assert!(boxes(stbl).iter().any(|(t, _)| *t == FourCc(*b"co64")));
    assert_eq!(moov_of(&moov)[0].samples[1].offset, 4_500_000_000);
}

#[test]
fn a_track_longer_than_a_32_bit_count_of_ticks_has_a_64_bit_media_duration() {
    // 80 minutes in microseconds is past u32::MAX.
    let v = [
        sample(0, 10, 0, 2_400_000_000, 0, true),
        sample(10, 10, 2_400_000_000, 2_400_000_000, 0, true),
    ];
    let head = single(&video(), &v, &[], 20);
    let mdhd = find_path(&head, &[b"moov", b"trak", b"mdia", b"mdhd"]);
    assert_eq!(mdhd[0], 1);
    // Version and flags, 64-bit creation and modification, timescale, 64-bit duration.
    assert_eq!(
        u32::from_be_bytes(mdhd[20..24].try_into().unwrap()),
        1_000_000
    );
    assert_eq!(
        u64::from_be_bytes(mdhd[24..32].try_into().unwrap()),
        4_800_000_000
    );
    assert_eq!(moov_of(&head)[0].info.time_base.den, 1_000_000);
}

#[test]
fn only_an_audio_track_has_a_volume() {
    // ISO/IEC 14496-12 § 8.3.2: the `tkhd` volume is 1.0 for audio and 0 for any other track.
    let tkhd_volume = |track: &Track| {
        let head = single(track, &[], &[], 0);
        let moov = moov_body(&head);
        let trak = find_path(moov, &[b"trak"]);
        let tkhd = find_path(trak, &[b"tkhd"]);
        // version + flags, 2 times, track id, reserved, duration, 8 reserved, layer, group: 36 bytes.
        u16::from_be_bytes([tkhd[36], tkhd[37]])
    };
    assert_eq!(tkhd_volume(&video()), 0);
    assert_eq!(tkhd_volume(&audio()), 0x0100);
}
