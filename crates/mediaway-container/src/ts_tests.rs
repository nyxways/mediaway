//! Unit tests for the MPEG-TS facade adapter (sibling of `ts.rs`).

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

const VIDEO_PID: u16 = 0x100;
const AUDIO_PID: u16 = 0x101;

fn streams() -> Vec<ElementaryStream> {
    vec![
        ElementaryStream {
            pid: VIDEO_PID,
            stream_type: StreamType::H264,
        },
        ElementaryStream {
            pid: AUDIO_PID,
            stream_type: StreamType::Aac,
        },
    ]
}

#[test]
fn mux_then_demux_roundtrips_two_streams() {
    let mut mux = Muxer::new(1, 0x1000, &streams()).expect("valid pids");
    let mut bytes = Vec::new();
    mux.write_pat_pmt(&mut bytes);
    mux.write_access_unit(
        VIDEO_PID,
        &[0, 0, 0, 1, 0x65, 0x88],
        90_000,
        None,
        true,
        &mut bytes,
    )
    .expect("video au");
    mux.write_access_unit(
        AUDIO_PID,
        &[1, 2, 3, 4],
        90_500,
        Some(90_000),
        false,
        &mut bytes,
    )
    .expect("audio au");
    // A real player needs the *next* PES packet on the same PID (or `finish`)
    // to confirm the previous one's boundary — write a trailing marker AU.
    mux.write_access_unit(
        VIDEO_PID,
        &[0, 0, 0, 1, 0x41],
        90_033,
        None,
        false,
        &mut bytes,
    )
    .expect("video au 2");

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);

    let video = demux.poll_packet().expect("video packet");
    assert_eq!(video.stream_id, u32::from(VIDEO_PID));
    assert_eq!(video.pts, 90_000);
    assert!(video.is_keyframe);
    assert_eq!(&video.payload[..], &[0, 0, 0, 1, 0x65, 0x88]);

    let streams = demux.streams();
    assert_eq!(streams.len(), 2);
    assert!(streams.iter().any(|s| s.codec() == CodecKind::H264));
    assert!(streams.iter().any(|s| s.codec() == CodecKind::Aac));
    assert_eq!(streams[0].time_base(), TS_TIME_BASE);
}

#[test]
fn finish_emits_the_final_pending_access_unit() {
    let mut mux = Muxer::new(1, 0x1000, &streams()).expect("valid pids");
    let mut bytes = Vec::new();
    mux.write_pat_pmt(&mut bytes);
    mux.write_access_unit(VIDEO_PID, &[9, 9, 9], 90_000, None, true, &mut bytes)
        .expect("video au");

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    // No trailing AU on this PID — poll_packet alone can't confirm the PES
    // boundary yet.
    assert!(demux.poll_packet().is_none());

    let finished = demux.finish();
    assert_eq!(finished.len(), 1);
    assert_eq!(&finished[0].payload[..], &[9, 9, 9]);
}

#[test]
fn invalid_pid_is_rejected() {
    let bad = [ElementaryStream {
        pid: 1, // reserved
        stream_type: StreamType::H264,
    }];
    assert!(matches!(
        Muxer::new(1, 0x1000, &bad),
        Err(Error::InvalidPid(1))
    ));
}
