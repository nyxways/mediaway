#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use std::time::Duration;

use mediaway_common::{Bytes, Packet, Rational};

use super::{PacketMeta, ReplayError, ReplayRing, StoredPayload};

const VIDEO: u32 = 1;
const AUDIO: u32 = 2;
const MS: Rational = Rational::new(1, 1000);

fn packet(stream_id: u32, dts: i64, pts: i64, key: bool, size: usize) -> Packet {
    Packet {
        stream_id,
        pts,
        dts,
        duration: 0,
        is_keyframe: key,
        is_discard: false,
        payload: Bytes::from(vec![0u8; size]),
    }
}

/// One video frame per second on a millisecond timebase, a keyframe every `gop` frames.
fn video_ring(window_secs: u64, frames: i64, gop: i64) -> ReplayRing {
    let mut ring = ReplayRing::new(VIDEO, MS, Duration::from_secs(window_secs)).unwrap();
    for i in 0..frames {
        ring.push(packet(VIDEO, i * 1000, i * 1000, i % gop == 0, 10))
            .unwrap();
    }
    ring
}

fn dts(ring: &ReplayRing, span_secs: u64) -> Vec<i64> {
    ring.clip_last(Duration::from_secs(span_secs))
        .unwrap()
        .packets()
        .map(|p| p.dts)
        .collect()
}

#[test]
fn eviction_keeps_a_cut_point_at_or_before_the_window_start() {
    // 30 frames (t = 0..29 s), keyframes at 0, 5, 10, 15, 20, 25, window 10 s. The window
    // starts at 19 s, so the keyframe at 15 s must survive and the one at 10 s must not.
    let ring = video_ring(10, 30, 5);
    assert_eq!(ring.span(), Duration::from_secs(14));
    assert_eq!(ring.bytes(), 15 * 10, "frames 15..29 held");
    assert_eq!(dts(&ring, 10).first(), Some(&0));
    assert_eq!(dts(&ring, 10).len(), 15);
}

#[test]
fn a_clip_starts_at_the_keyframe_before_the_requested_start_never_after() {
    let ring = video_ring(60, 30, 5);
    // newest = 29 s. Asking for 12 s wants 17 s; the keyframe at or before it is 15 s.
    let clip = ring.clip_last(Duration::from_secs(12)).unwrap();
    assert_eq!(clip.anchor_origin(), 15_000);
    assert_eq!(clip.duration(), Duration::from_secs(14));
    // Exactly on a keyframe: asking for 9 s wants 20 s, which is one.
    assert_eq!(
        ring.clip_last(Duration::from_secs(9))
            .unwrap()
            .anchor_origin(),
        20_000
    );
}

#[test]
fn asking_for_more_than_is_held_starts_at_the_oldest_keyframe() {
    let ring = video_ring(10, 30, 5);
    assert_eq!(
        ring.clip_last(Duration::from_secs(600))
            .unwrap()
            .anchor_origin(),
        15_000
    );
}

#[test]
fn timestamps_are_rebased_so_the_cut_keyframe_decodes_at_zero() {
    let ring = video_ring(60, 30, 5);
    let first = ring
        .clip_last(Duration::from_secs(12))
        .unwrap()
        .packets()
        .next()
        .unwrap();
    assert!(first.is_keyframe);
    assert_eq!((first.dts, first.pts), (0, 0));
}

#[test]
fn the_cut_is_in_decode_order_and_leading_pictures_are_dropped() {
    // Decode order of a reordering encoder, open-GOP style. `L*` are leading pictures of the
    // second keyframe: decoded after it, presented before it, and possibly referencing the
    // first GOP. A cut at K2 must keep K2 and what follows, and drop L1/L2.
    //
    //   name  dts  pts
    //   K1      0   20
    //   B       1   10
    //   P       2   40
    //   K2      3   60
    //   L1      4   45
    //   L2      5   50
    //   P       6   80
    let mut ring = ReplayRing::new(VIDEO, MS, Duration::from_secs(60)).unwrap();
    for (dts, pts, key) in [
        (0, 20, true),
        (1, 10, false),
        (2, 40, false),
        (3, 60, true),
        (4, 45, false),
        (5, 50, false),
        (6, 80, false),
    ] {
        ring.push(packet(VIDEO, dts, pts, key, 1)).unwrap();
    }
    // newest dts = 6 ms. A 3 ms clip wants dts 3 → K2.
    let clip = ring.clip_last(Duration::from_millis(3)).unwrap();
    let got: Vec<(i64, i64)> = clip.packets().map(|p| (p.dts, p.pts)).collect();
    assert_eq!(
        got,
        vec![(0, 57), (3, 77)],
        "K2 and the P after it, rebased by K2's dts"
    );
}

#[test]
fn a_second_stream_is_cut_by_time_interleaved_and_rebased_in_its_own_timebase() {
    // Video on a 90 kHz clock, keyframe every 2 s at 1 fps. Audio on 48 kHz, one packet per
    // 0.5 s. Both start at t = 0.
    let v = Rational::new(1, 90_000);
    let a = Rational::new(1, 48_000);
    let mut ring = ReplayRing::new(VIDEO, v, Duration::from_secs(60)).unwrap();
    ring.add_stream(AUDIO, a).unwrap();
    for half in 0..12i64 {
        if half % 2 == 0 {
            let s = half / 2;
            ring.push(packet(VIDEO, s * 90_000, s * 90_000, s % 2 == 0, 1))
                .unwrap();
        }
        ring.push(packet(AUDIO, half * 24_000, half * 24_000, true, 1))
            .unwrap();
    }
    // newest = 5.5 s (audio). 2 s wants 3.5 s → keyframe at 2 s. Cut: video 2..5 s,
    // audio 2.0..5.5 s, all shifted by 2 s.
    let clip = ring.clip_last(Duration::from_secs(2)).unwrap();
    let got: Vec<(u32, i64)> = clip.packets().map(|p| (p.stream_id, p.dts)).collect();
    assert_eq!(
        got,
        vec![
            (VIDEO, 0),
            (AUDIO, 0),
            (AUDIO, 24_000),
            (VIDEO, 90_000),
            (AUDIO, 48_000),
            (AUDIO, 72_000),
            (VIDEO, 180_000),
            (AUDIO, 96_000),
            (AUDIO, 120_000),
            (VIDEO, 270_000),
            (AUDIO, 144_000),
            (AUDIO, 168_000),
        ]
    );
}

#[test]
fn the_byte_ceiling_evicts_inside_the_window_but_keeps_the_newest_gop() {
    let mut ring = ReplayRing::new(VIDEO, MS, Duration::from_secs(3600))
        .unwrap()
        .with_max_bytes(120);
    for i in 0..30 {
        ring.push(packet(VIDEO, i * 1000, i * 1000, i % 5 == 0, 10))
            .unwrap();
    }
    // 10 bytes a frame, 50 a GOP. The ceiling is checked after each push, so the ring
    // settles on whole GOPs at 20 and 25 s.
    assert_eq!(ring.bytes(), 100, "GOPs at 20 and 25 s");
    assert_eq!(
        ring.clip_last(Duration::from_secs(3600))
            .unwrap()
            .anchor_origin(),
        20_000
    );

    // One GOP bigger than the ceiling on its own still stays.
    let mut ring = ReplayRing::new(VIDEO, MS, Duration::from_secs(3600))
        .unwrap()
        .with_max_bytes(5);
    for i in 0..3 {
        ring.push(packet(VIDEO, i * 1000, i * 1000, i == 0, 10))
            .unwrap();
    }
    assert_eq!(ring.bytes(), 30);
    assert!(ring.clip_last(Duration::from_secs(1)).is_some());
}

#[test]
fn anchor_packets_before_the_first_keyframe_are_dropped() {
    let mut ring = ReplayRing::new(VIDEO, MS, Duration::from_secs(60)).unwrap();
    ring.push(packet(VIDEO, 0, 0, false, 10)).unwrap();
    assert!(ring.clip_last(Duration::from_secs(1)).is_none());
    assert_eq!((ring.bytes(), ring.span()), (0, Duration::ZERO));
    ring.push(packet(VIDEO, 1000, 1000, true, 10)).unwrap();
    assert_eq!(dts(&ring, 60), vec![0]);
}

#[test]
fn an_empty_ring_has_no_clip() {
    let ring = ReplayRing::new(VIDEO, MS, Duration::from_secs(60)).unwrap();
    assert!(ring.clip_last(Duration::from_secs(1)).is_none());
}

#[test]
fn decode_order_is_enforced_per_stream() {
    let mut ring = ReplayRing::new(VIDEO, MS, Duration::from_secs(60)).unwrap();
    ring.add_stream(AUDIO, MS).unwrap();
    ring.push(packet(VIDEO, 1000, 1000, true, 1)).unwrap();
    // Another stream may be behind the anchor; only a stream's own order matters.
    ring.push(packet(AUDIO, 500, 500, true, 1)).unwrap();
    assert_eq!(
        ring.push(packet(VIDEO, 999, 999, false, 1)),
        Err(ReplayError::OutOfOrder {
            stream_id: VIDEO,
            previous: 1000,
            got: 999
        })
    );
}

#[test]
fn bad_streams_are_refused() {
    let mut ring = ReplayRing::new(VIDEO, MS, Duration::from_secs(60)).unwrap();
    assert_eq!(
        ring.push(packet(9, 0, 0, true, 1)),
        Err(ReplayError::UnknownStream { stream_id: 9 })
    );
    assert_eq!(
        ring.add_stream(VIDEO, MS),
        Err(ReplayError::DuplicateStream { stream_id: VIDEO })
    );
    assert!(matches!(
        ring.add_stream(AUDIO, Rational::new(1, 0)),
        Err(ReplayError::BadTimebase { .. })
    ));
    assert!(ReplayRing::new(VIDEO, Rational::new(0, 1000), Duration::from_secs(1)).is_err());
}

/// A reordering video stream (90 kHz, 30 fps, keyframe every 12 frames, B-frames) with audio
/// (48 kHz, 1024-sample packets) interleaved by time, starting with three anchor packets before
/// the first keyframe. Payload `i` is `8 + (i * 37) % 300` bytes whose first 8 are `i`.
fn av_sequence() -> Vec<Packet> {
    let mut out = Vec::new();
    let mut audio_dts = 0i64;
    let mut index = 0u64;
    let mut push = |out: &mut Vec<Packet>, stream_id, dts, pts, key| {
        let size = 8 + usize::try_from(index * 37 % 300).unwrap();
        let mut payload = vec![0u8; size];
        payload[..8].copy_from_slice(&index.to_le_bytes());
        index += 1;
        out.push(Packet {
            stream_id,
            pts,
            dts,
            duration: 0,
            is_keyframe: key,
            is_discard: false,
            payload: Bytes::from(payload),
        });
    };
    for frame in 0..600i64 {
        // Decode order P, B, B: the P presents after the two B-frames that follow it. The
        // keyframes (every 12th frame from frame 3) therefore have leading pictures.
        let pts = match frame % 3 {
            0 => frame + 2,
            _ => frame - 1,
        } + 1;
        let dts = frame;
        push(&mut out, VIDEO, dts * 3000, pts * 3000, frame % 12 == 3);
        while audio_dts * 90_000 <= dts * 3000 * 48_000 {
            push(&mut out, AUDIO, audio_dts, audio_dts, true);
            audio_dts += 1024;
        }
    }
    out
}

fn index_of(payload: &Bytes) -> usize {
    usize::try_from(u64::from_le_bytes(payload[..8].try_into().unwrap())).unwrap()
}

#[test]
fn a_ring_of_stored_payloads_evicts_and_cuts_exactly_like_a_ring_of_bytes() {
    let v = Rational::new(1, 90_000);
    let a = Rational::new(1, 48_000);
    let packets = av_sequence();
    // Where each packet "was written": 50 packets per file, back to back.
    let mut handles = Vec::new();
    let mut offset = 0u64;
    for (i, p) in packets.iter().enumerate() {
        if i % 50 == 0 {
            offset = 0;
        }
        let len = u32::try_from(p.payload.len()).unwrap();
        handles.push(StoredPayload {
            file: u32::try_from(i / 50).unwrap(),
            offset,
            len,
        });
        offset += u64::from(len);
    }

    for max_bytes in [None, Some(20_000)] {
        let mut bytes_ring = ReplayRing::new(VIDEO, v, Duration::from_secs(3)).unwrap();
        let mut stored_ring =
            ReplayRing::<StoredPayload>::for_payload(VIDEO, v, Duration::from_secs(3)).unwrap();
        if let Some(max) = max_bytes {
            bytes_ring = bytes_ring.with_max_bytes(max);
            stored_ring = stored_ring.with_max_bytes(max);
        }
        bytes_ring.add_stream(AUDIO, a).unwrap();
        stored_ring.add_stream(AUDIO, a).unwrap();

        for (i, packet) in packets.iter().enumerate() {
            stored_ring
                .push_entry(PacketMeta::from(packet), handles[i])
                .unwrap();
            bytes_ring.push(packet.clone()).unwrap();

            assert_eq!(stored_ring.bytes(), bytes_ring.bytes(), "packet {i}");
            assert_eq!(stored_ring.span(), bytes_ring.span(), "packet {i}");
            let held: Vec<StoredPayload> = bytes_ring
                .payloads()
                .map(|b| handles[index_of(b)])
                .collect();
            assert!(stored_ring.payloads().copied().eq(held), "packet {i}");
            if i % 25 != 0 {
                continue;
            }
            for span_ms in [0, 700, 1500, 2900, 10_000] {
                let span = Duration::from_millis(span_ms);
                let (Some(want), Some(got)) =
                    (bytes_ring.clip_last(span), stored_ring.clip_last(span))
                else {
                    assert!(bytes_ring.clip_last(span).is_none());
                    assert!(stored_ring.clip_last(span).is_none());
                    continue;
                };
                assert_eq!(got.anchor_origin(), want.anchor_origin());
                assert_eq!(got.duration(), want.duration());
                let want: Vec<(PacketMeta, StoredPayload)> = want
                    .packets()
                    .map(|p| (PacketMeta::from(&p), handles[index_of(&p.payload)]))
                    .collect();
                let got: Vec<(PacketMeta, StoredPayload)> =
                    got.entries().map(|(m, h)| (m, *h)).collect();
                assert!(!got.is_empty());
                assert_eq!(got, want, "packet {i}, span {span_ms} ms");
            }
        }
        // Eviction really happened, so old files are free: none of the first ones is referred to.
        let oldest_file = stored_ring.payloads().map(|h| h.file).min().unwrap();
        assert!(oldest_file > 0, "max_bytes {max_bytes:?}");
    }
}

#[test]
fn entries_and_packets_agree_for_bytes() {
    let ring = video_ring(60, 30, 5);
    let clip = ring.clip_last(Duration::from_secs(12)).unwrap();
    let from_entries: Vec<Packet> = clip
        .entries()
        .map(|(meta, payload)| meta.into_packet(payload.clone()))
        .collect();
    assert_eq!(from_entries, clip.packets().collect::<Vec<_>>());
}

/// The flow a file-writing caller runs: mux with placements, hold `StoredPayload`s, and read a
/// clip's bytes back out of what the muxer emitted.
#[cfg(feature = "mux")]
#[test]
fn stored_payloads_from_mp4_placements_read_back_as_the_pushed_packets() {
    use std::collections::VecDeque;

    use mediaway_common::{CodecKind, StreamInfo, VideoGeometry};

    use crate::mp4;

    let v = Rational::new(1, 90_000);
    let a = Rational::new(1, 48_000);
    let packets = av_sequence();
    let mut open = mp4::Muxer::with_fragment_batch(8).with_placements();
    open.add_track(StreamInfo::Video {
        id: VIDEO,
        codec: CodecKind::H264,
        time_base: v,
        geometry: VideoGeometry {
            width: 64,
            height: 64,
        },
        extra_data: Bytes::new(),
    })
    .unwrap();
    open.add_track(StreamInfo::Audio {
        id: AUDIO,
        codec: CodecKind::Opus,
        time_base: a,
        extra_data: Bytes::new(),
        sample_rate: 48_000,
        channels: 2,
    })
    .unwrap();
    let mut mux = open.begin();
    let mut ring = ReplayRing::<StoredPayload>::for_payload(VIDEO, v, Duration::from_secs(3))
        .unwrap()
        .with_max_bytes(30_000);
    ring.add_stream(AUDIO, a).unwrap();

    // The file, and packets muxed but not yet placed, per stream.
    let mut file = Vec::new();
    let mut waiting: [VecDeque<PacketMeta>; 2] = Default::default();
    let place = |mux: &mut mp4::Muxer<mp4::Live>,
                 ring: &mut ReplayRing<StoredPayload>,
                 file: &mut Vec<u8>,
                 waiting: &mut [VecDeque<PacketMeta>; 2]| {
        mux.poll_bytes(file);
        let mut placements = Vec::new();
        mux.poll_placements(&mut placements);
        for p in placements {
            let meta = waiting[usize::from(p.track_id == AUDIO)]
                .pop_front()
                .unwrap();
            assert_eq!((meta.stream_id, meta.dts), (p.track_id, p.dts));
            let stored = StoredPayload {
                file: 0,
                offset: p.offset,
                len: p.len,
            };
            ring.push_entry(meta, stored).unwrap();
        }
    };
    for packet in &packets {
        mux.push_packet(packet).unwrap();
        waiting[usize::from(packet.stream_id == AUDIO)].push_back(PacketMeta::from(packet));
        place(&mut mux, &mut ring, &mut file, &mut waiting);
    }
    mux.flush();
    place(&mut mux, &mut ring, &mut file, &mut waiting);
    assert!(
        waiting.iter().all(VecDeque::is_empty),
        "every packet placed"
    );

    let clip = ring.clip_last(Duration::from_secs(2)).unwrap();
    let mut streams = [0usize; 2];
    for (meta, stored) in clip.entries() {
        let start = usize::try_from(stored.offset).unwrap();
        let bytes = &file[start..start + stored.len as usize];
        let original =
            &packets[usize::try_from(u64::from_le_bytes(bytes[..8].try_into().unwrap())).unwrap()];
        assert_eq!(bytes, original.payload.as_ref());
        assert_eq!(meta.stream_id, original.stream_id);
        assert_eq!(
            meta.pts - meta.dts,
            original.pts - original.dts,
            "rebased as a whole"
        );
        if meta.stream_id == VIDEO {
            assert_eq!(meta.dts + clip.anchor_origin(), original.dts);
        }
        streams[usize::from(meta.stream_id == AUDIO)] += 1;
    }
    assert!(streams.iter().all(|&n| n > 10), "{streams:?}");
}
