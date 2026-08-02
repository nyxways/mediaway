//! Integration: public API mux → demux round trip (PAT/PMT + two elementary streams).

#![forbid(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration tests may unwrap"
)]

use mpeg_ts::{Demuxer, ElementaryStream, Muxer, StreamType};

#[test]
fn single_program_two_stream_roundtrip_via_public_api() {
    let streams = [
        ElementaryStream {
            pid: 256,
            stream_type: StreamType::H264,
        },
        ElementaryStream {
            pid: 257,
            stream_type: StreamType::Aac,
        },
    ];
    let mut mux = Muxer::new(1, 4096, &streams).expect("mux");
    let mut bytes = Vec::new();
    mux.write_pat_pmt(&mut bytes);
    mux.write_access_unit(256, b"keyframe", 0, Some(0), true, &mut bytes)
        .expect("video access unit");
    mux.write_access_unit(257, b"audio frame", 0, None, false, &mut bytes)
        .expect("audio access unit");
    // A trailing access unit per PID confirms the prior one via the next PUSI.
    mux.write_access_unit(256, b"interframe", 3_000, Some(3_000), false, &mut bytes)
        .expect("second video access unit");

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);

    let mut got = Vec::new();
    while let Some(unit) = demux.poll_access_unit().expect("poll") {
        got.push(unit);
    }
    got.extend(demux.finish());

    assert_eq!(demux.streams(), streams.as_slice());
    assert_eq!(got.len(), 3);
    let video: Vec<_> = got.iter().filter(|u| u.pid == 256).collect();
    let audio: Vec<_> = got.iter().filter(|u| u.pid == 257).collect();
    assert_eq!(video.len(), 2);
    assert_eq!(audio.len(), 1);
    assert_eq!(&video[0].data[..], b"keyframe");
    assert!(video[0].random_access);
    assert_eq!(&audio[0].data[..], b"audio frame");
}
