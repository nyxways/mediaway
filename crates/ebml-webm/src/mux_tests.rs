//! Unit tests for the `WebM` muxer (sibling of `mux.rs`) — round-trips
//! through [`crate::Demuxer`], the strongest correctness check available
//! without an external oracle for a format this crate also writes.

#![cfg(test)]
#![allow(clippy::unwrap_used, reason = "test module may unwrap")]

use super::{Muxer, Open};
use crate::types::TrackInfo;
use crate::{Demuxer, MuxError};

fn video_track(track_number: u64) -> TrackInfo {
    TrackInfo {
        track_number,
        track_type: 1,
        codec_id: "V_VP9".to_string(),
        codec_private: None,
        width: 1920,
        height: 1080,
        sample_rate: 8000.0,
        channels: 1,
    }
}

fn audio_track(track_number: u64) -> TrackInfo {
    TrackInfo {
        track_number,
        track_type: 2,
        codec_id: "A_OPUS".to_string(),
        codec_private: None,
        width: 0,
        height: 0,
        sample_rate: 48000.0,
        channels: 2,
    }
}

#[test]
fn add_track_rejects_zero_track_number() {
    let mut m = Muxer::<Open>::new();
    assert_eq!(
        m.add_track(video_track(0)),
        Err(MuxError::InvalidTrackNumber)
    );
}

#[test]
fn add_track_rejects_duplicate() {
    let mut m = Muxer::<Open>::new();
    m.add_track(video_track(1)).unwrap();
    assert_eq!(
        m.add_track(video_track(1)),
        Err(MuxError::DuplicateTrack(1))
    );
}

#[test]
fn push_frame_rejects_unregistered_track() {
    let mut m = Muxer::<Open>::new();
    m.add_track(video_track(1)).unwrap();
    let mut live = m.begin();
    assert_eq!(
        live.push_frame(99, 0, true, b"data"),
        Err(MuxError::UnknownTrack(99))
    );
}

#[test]
fn single_video_track_round_trips_through_demuxer() {
    let mut m = Muxer::<Open>::new();
    m.add_track(video_track(1)).unwrap();
    let mut live = m.begin();

    live.push_frame(1, 0, true, b"keyframe-payload").unwrap();
    live.push_frame(1, 33, false, b"delta-payload-1").unwrap();
    live.push_frame(1, 66, false, b"delta-payload-2").unwrap();
    live.flush();

    let mut bytes = Vec::new();
    live.poll_bytes(&mut bytes);
    assert!(!bytes.is_empty());

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);

    let tracks = demux.streams();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].track_number, 1);
    assert_eq!(tracks[0].codec_id, "V_VP9");
    assert_eq!(tracks[0].width, 1920);
    assert_eq!(tracks[0].height, 1080);

    let f0 = demux.poll_frame().unwrap();
    assert_eq!(f0.track_number, 1);
    assert_eq!(f0.timecode, 0);
    assert!(f0.is_keyframe);
    assert_eq!(&f0.payload[..], b"keyframe-payload");

    let f1 = demux.poll_frame().unwrap();
    assert_eq!(f1.timecode, 33);
    assert!(!f1.is_keyframe);
    assert_eq!(&f1.payload[..], b"delta-payload-1");

    let f2 = demux.poll_frame().unwrap();
    assert_eq!(f2.timecode, 66);
    assert_eq!(&f2.payload[..], b"delta-payload-2");

    assert!(demux.poll_frame().is_none());
}

#[test]
fn multi_track_audio_video_round_trips() {
    let mut m = Muxer::<Open>::new();
    m.add_track(video_track(1)).unwrap();
    m.add_track(audio_track(2)).unwrap();
    let mut live = m.begin();

    live.push_frame(1, 0, true, b"v0").unwrap();
    live.push_frame(2, 0, true, b"a0").unwrap();
    live.push_frame(1, 40, false, b"v1").unwrap();
    live.push_frame(2, 20, true, b"a1").unwrap();
    live.flush();

    let mut bytes = Vec::new();
    live.poll_bytes(&mut bytes);

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);

    let tracks = demux.streams();
    assert_eq!(tracks.len(), 2);
    let audio = tracks.iter().find(|t| t.track_number == 2).unwrap();
    assert_eq!(audio.codec_id, "A_OPUS");
    assert_eq!(audio.channels, 2);
    assert!((audio.sample_rate - 48000.0).abs() < f64::EPSILON);

    let mut frames = Vec::new();
    while let Some(f) = demux.poll_frame() {
        frames.push((f.track_number, f.timecode, f.payload.to_vec()));
    }
    assert_eq!(frames.len(), 4);
    assert!(frames.contains(&(1, 0, b"v0".to_vec())));
    assert!(frames.contains(&(2, 0, b"a0".to_vec())));
    assert!(frames.contains(&(1, 40, b"v1".to_vec())));
    assert!(frames.contains(&(2, 20, b"a1".to_vec())));
}

#[test]
fn cluster_batch_boundary_forces_new_cluster_and_still_round_trips() {
    let mut m = Muxer::<Open>::with_options(1_000_000, 4);
    m.add_track(video_track(1)).unwrap();
    let mut live = m.begin();

    for i in 0..10i64 {
        live.push_frame(1, i * 10, i == 0, format!("f{i}").as_bytes())
            .unwrap();
    }
    live.flush();

    let mut bytes = Vec::new();
    live.poll_bytes(&mut bytes);

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    let mut count = 0;
    while let Some(f) = demux.poll_frame() {
        assert_eq!(f.timecode, count * 10);
        count += 1;
    }
    assert_eq!(count, 10);
}

#[test]
fn large_timecode_jump_forces_new_cluster_and_still_round_trips() {
    let mut m = Muxer::<Open>::new();
    m.add_track(video_track(1)).unwrap();
    let mut live = m.begin();

    // A relative offset over i16::MAX (32767) from the same cluster base
    // must force a new cluster rather than truncating/wrapping.
    live.push_frame(1, 0, true, b"a").unwrap();
    live.push_frame(1, 100_000, true, b"b").unwrap();
    live.flush();

    let mut bytes = Vec::new();
    live.poll_bytes(&mut bytes);

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    let f0 = demux.poll_frame().unwrap();
    assert_eq!(f0.timecode, 0);
    let f1 = demux.poll_frame().unwrap();
    assert_eq!(f1.timecode, 100_000);
    assert!(demux.poll_frame().is_none());
}

#[test]
fn empty_stream_no_frames_pushed_still_parses_as_valid_header() {
    let m = Muxer::<Open>::new();
    let mut live = m.begin();
    live.flush();
    let mut bytes = Vec::new();
    live.poll_bytes(&mut bytes);
    assert!(!bytes.is_empty());

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    assert!(demux.streams().is_empty());
    assert!(demux.poll_frame().is_none());
}

#[test]
fn push_laced_frames_rejects_empty_payloads() {
    let mut m = Muxer::<Open>::new();
    m.add_track(video_track(1)).unwrap();
    let mut live = m.begin();
    assert_eq!(
        live.push_laced_frames(1, 0, true, &[]),
        Err(MuxError::EmptyLace)
    );
}

#[test]
fn push_laced_frames_rejects_too_many_sub_frames() {
    let mut m = Muxer::<Open>::new();
    m.add_track(video_track(1)).unwrap();
    let mut live = m.begin();
    let payloads: Vec<&[u8]> = std::iter::repeat_n(&b"x"[..], 257).collect();
    assert_eq!(
        live.push_laced_frames(1, 0, true, &payloads),
        Err(MuxError::LaceTooLarge(257))
    );
}

#[test]
fn push_laced_frames_single_payload_matches_plain_simple_block() {
    let mut m = Muxer::<Open>::new();
    m.add_track(video_track(1)).unwrap();
    let mut live = m.begin();
    live.push_laced_frames(1, 0, true, &[b"solo"]).unwrap();
    live.flush();

    let mut bytes = Vec::new();
    live.poll_bytes(&mut bytes);
    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    let f0 = demux.poll_frame().unwrap();
    assert_eq!(&f0.payload[..], b"solo");
    assert!(demux.poll_frame().is_none());
}

#[test]
fn push_laced_frames_round_trips_multiple_sub_frames_via_demuxer() {
    let mut m = Muxer::<Open>::new();
    m.add_track(video_track(1)).unwrap();
    let mut live = m.begin();

    live.push_laced_frames(1, 0, true, &[b"a", b"bb", b"ccc"])
        .unwrap();
    live.push_frame(1, 10, false, b"unlaced").unwrap();
    live.flush();

    let mut bytes = Vec::new();
    live.poll_bytes(&mut bytes);
    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);

    let f0 = demux.poll_frame().unwrap();
    assert_eq!(&f0.payload[..], b"a");
    assert_eq!(f0.timecode, 0);
    assert!(f0.is_keyframe);
    let f1 = demux.poll_frame().unwrap();
    assert_eq!(&f1.payload[..], b"bb");
    assert_eq!(f1.timecode, 0, "laced sub-frames share one timecode");
    let f2 = demux.poll_frame().unwrap();
    assert_eq!(&f2.payload[..], b"ccc");
    assert_eq!(f2.timecode, 0);
    let f3 = demux.poll_frame().unwrap();
    assert_eq!(&f3.payload[..], b"unlaced");
    assert_eq!(f3.timecode, 10);
    assert!(demux.poll_frame().is_none());
}

#[test]
fn push_laced_frames_round_trips_many_sub_frames_of_varying_size() {
    let mut m = Muxer::<Open>::new();
    m.add_track(video_track(1)).unwrap();
    let mut live = m.begin();

    let sizes = [1usize, 300, 2, 5000, 1];
    let owned: Vec<Vec<u8>> = sizes.iter().map(|&n| vec![0xAB; n]).collect();
    let payloads: Vec<&[u8]> = owned.iter().map(Vec::as_slice).collect();
    live.push_laced_frames(1, 5, true, &payloads).unwrap();
    live.flush();

    let mut bytes = Vec::new();
    live.poll_bytes(&mut bytes);
    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);

    for &size in &sizes {
        let f = demux.poll_frame().unwrap();
        assert_eq!(f.payload.len(), size);
        assert_eq!(f.timecode, 5);
    }
    assert!(demux.poll_frame().is_none());
}

#[test]
fn chunked_feed_still_round_trips() {
    let mut m = Muxer::<Open>::new();
    m.add_track(video_track(1)).unwrap();
    let mut live = m.begin();
    for i in 0..5i64 {
        live.push_frame(1, i * 33, i == 0, format!("payload-{i}").as_bytes())
            .unwrap();
    }
    live.flush();

    let mut bytes = Vec::new();
    live.poll_bytes(&mut bytes);

    let mut demux = Demuxer::new();
    for chunk in bytes.chunks(3) {
        demux.push_bytes(chunk);
    }
    let mut count = 0;
    while let Some(f) = demux.poll_frame() {
        assert_eq!(f.timecode, count * 33);
        count += 1;
    }
    assert_eq!(count, 5);
}
