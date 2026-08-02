//! Integration round-trip: `Muxer`'s public command + audio/video methods, decoded back via
//! `Demuxer`, using only `rtmp`'s public API (Tier 2, `docs/conventions/testing.md`).

#![allow(clippy::unwrap_used, reason = "integration test")]

use rtmp::{Demuxer, Muxer, OnMetaData};

#[test]
fn publish_session_messages_round_trip_through_demuxer() {
    let mut mux = Muxer::new(256);
    let mut wire = Vec::new();

    mux.write_connect("live", "rtmp://example.invalid/live", &mut wire);
    mux.write_create_stream(&mut wire);
    mux.write_publish("my-stream-key", &mut wire);
    mux.write_metadata(
        &OnMetaData {
            width: Some(1280.0),
            height: Some(720.0),
            framerate: Some(30.0),
            videocodecid: Some(7.0),
            audiocodecid: Some(10.0),
        },
        &mut wire,
    );
    mux.push_video_data(&[0, 1, 2, 3, 4, 5], 0, &mut wire);
    mux.push_audio_data(&[9, 9, 9], 33, &mut wire);
    mux.push_video_data(
        &(0..300u16)
            .map(|i| u8::try_from(i % 256).unwrap_or(0))
            .collect::<Vec<_>>(),
        66,
        &mut wire,
    );

    let mut demux = Demuxer::new();
    demux.push_bytes(&wire);

    let mut messages = Vec::new();
    while let Some(msg) = demux.poll_message().unwrap() {
        messages.push(msg);
    }

    // Set Chunk Size + connect + createStream + publish + onMetaData + 3 media messages.
    assert_eq!(messages.len(), 8);
    assert_eq!(messages[0].0, 1); // Set Chunk Size
    assert_eq!(messages[1].0, 20); // connect
    assert_eq!(messages[2].0, 20); // createStream
    assert_eq!(messages[3].0, 20); // publish
    assert_eq!(messages[4].0, 18); // onMetaData

    let video1 = &messages[5];
    assert_eq!((video1.0, video1.1), (9, 0));
    assert_eq!(video1.2, vec![0, 1, 2, 3, 4, 5]);

    let audio1 = &messages[6];
    assert_eq!((audio1.0, audio1.1), (8, 33));
    assert_eq!(audio1.2, vec![9, 9, 9]);

    let video2 = &messages[7];
    assert_eq!((video2.0, video2.1), (9, 66));
    let expected: Vec<u8> = (0..300u16)
        .map(|i| u8::try_from(i % 256).unwrap_or(0))
        .collect();
    assert_eq!(video2.2, expected);
}

#[test]
fn fed_in_small_chunks_still_reassembles() {
    let mut mux = Muxer::new(64);
    let mut wire = Vec::new();
    mux.push_video_data(&vec![42u8; 500], 12345, &mut wire);

    let mut demux = Demuxer::new();
    let mut result = None;
    for chunk in wire.chunks(7) {
        demux.push_bytes(chunk);
        while let Some(msg) = demux.poll_message().unwrap() {
            if msg.0 == 9 {
                result = Some(msg);
            }
        }
    }
    let (message_type_id, timestamp_ms, payload) = result.unwrap();
    assert_eq!(message_type_id, 9);
    assert_eq!(timestamp_ms, 12345);
    assert_eq!(payload, vec![42u8; 500]);
}
