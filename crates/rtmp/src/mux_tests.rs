//! Unit tests for `Muxer`, decoding what it wrote back via `Demuxer`/`ChunkDecoder` to
//! verify message framing, and byte-comparing AMF0 command payloads against hand-built
//! expected sequences.

#![cfg(test)]
#![allow(clippy::unwrap_used, reason = "unit tests")]

use super::Muxer;
use crate::amf0;
use crate::demux::Demuxer;
use crate::types::OnMetaData;

#[test]
fn first_call_emits_set_chunk_size_before_connect() {
    let mut mux = Muxer::new(4096);
    let mut out = Vec::new();
    mux.write_connect("live", "rtmp://example.invalid/live", &mut out);

    let mut demux = Demuxer::new();
    demux.push_bytes(&out);
    let (t0, _, p0) = demux.poll_message().unwrap().unwrap();
    assert_eq!(t0, 1); // Set Chunk Size
    assert_eq!(p0, 4096u32.to_be_bytes());

    let (t1, _, _) = demux.poll_message().unwrap().unwrap();
    assert_eq!(t1, 20); // AMF0 command
    assert!(demux.poll_message().unwrap().is_none());
}

#[test]
fn set_chunk_size_emitted_only_once() {
    let mut mux = Muxer::new(1024);
    let mut out = Vec::new();
    mux.write_connect("live", "rtmp://example.invalid/live", &mut out);
    mux.write_create_stream(&mut out);

    let mut demux = Demuxer::new();
    demux.push_bytes(&out);
    let mut set_chunk_size_count = 0;
    while let Some((message_type_id, ..)) = demux.poll_message().unwrap() {
        if message_type_id == 1 {
            set_chunk_size_count += 1;
        }
    }
    assert_eq!(set_chunk_size_count, 1);
}

#[test]
fn write_connect_payload_matches_hand_built_amf0() {
    let mut mux = Muxer::new(128);
    let mut out = Vec::new();
    mux.write_connect("live", "rtmp://example.invalid/live", &mut out);

    let mut demux = Demuxer::new();
    demux.push_bytes(&out);
    let _ = demux.poll_message().unwrap().unwrap(); // Set Chunk Size
    let (message_type_id, timestamp_ms, payload) = demux.poll_message().unwrap().unwrap();
    assert_eq!(message_type_id, 20);
    assert_eq!(timestamp_ms, 0);

    let mut expected = Vec::new();
    amf0::write_string(&mut expected, "connect").unwrap();
    amf0::write_number(&mut expected, 1.0);
    amf0::write_object_start(&mut expected);
    amf0::write_property_name(&mut expected, "app").unwrap();
    amf0::write_string(&mut expected, "live").unwrap();
    amf0::write_property_name(&mut expected, "flashVer").unwrap();
    amf0::write_string(&mut expected, "MEDIAWAY/1,0,0,0").unwrap();
    amf0::write_property_name(&mut expected, "tcUrl").unwrap();
    amf0::write_string(&mut expected, "rtmp://example.invalid/live").unwrap();
    amf0::write_object_end(&mut expected);

    assert_eq!(payload, expected);
}

#[test]
fn write_publish_payload_matches_hand_built_amf0() {
    let mut mux = Muxer::new(128);
    let mut out = Vec::new();
    mux.write_publish("stream-key", &mut out);

    let mut demux = Demuxer::new();
    demux.push_bytes(&out);
    let _ = demux.poll_message().unwrap().unwrap(); // Set Chunk Size
    let (message_type_id, _, payload) = demux.poll_message().unwrap().unwrap();
    assert_eq!(message_type_id, 20);

    let mut expected = Vec::new();
    amf0::write_string(&mut expected, "publish").unwrap();
    amf0::write_number(&mut expected, 0.0);
    amf0::write_null(&mut expected);
    amf0::write_string(&mut expected, "stream-key").unwrap();
    amf0::write_string(&mut expected, "live").unwrap();

    assert_eq!(payload, expected);
}

#[test]
fn write_metadata_only_includes_set_fields() {
    let mut mux = Muxer::new(128);
    let mut out = Vec::new();
    let meta = OnMetaData {
        width: Some(1920.0),
        height: Some(1080.0),
        ..OnMetaData::default()
    };
    mux.write_metadata(&meta, &mut out);

    let mut demux = Demuxer::new();
    demux.push_bytes(&out);
    let _ = demux.poll_message().unwrap().unwrap(); // Set Chunk Size
    let (message_type_id, _, payload) = demux.poll_message().unwrap().unwrap();
    assert_eq!(message_type_id, 18); // AMF0 data

    let mut expected = Vec::new();
    amf0::write_string(&mut expected, "onMetaData").unwrap();
    amf0::write_ecma_array_start(&mut expected, 2);
    amf0::write_property_name(&mut expected, "width").unwrap();
    amf0::write_number(&mut expected, 1920.0);
    amf0::write_property_name(&mut expected, "height").unwrap();
    amf0::write_number(&mut expected, 1080.0);
    amf0::write_object_end(&mut expected);

    assert_eq!(payload, expected);
}

#[test]
fn push_video_and_audio_data_round_trip() {
    let mut mux = Muxer::new(128);
    let mut out = Vec::new();
    mux.push_video_data(&[1, 2, 3, 4], 1000, &mut out);
    mux.push_audio_data(&[9, 9], 1001, &mut out);

    let mut demux = Demuxer::new();
    demux.push_bytes(&out);
    let _ = demux.poll_message().unwrap().unwrap(); // Set Chunk Size (first out-appending call)
    let video_msg = demux.poll_message().unwrap().unwrap();
    let audio_msg = demux.poll_message().unwrap().unwrap();

    assert_eq!(video_msg, (9, 1000, vec![1, 2, 3, 4]));
    assert_eq!(audio_msg, (8, 1001, vec![9, 9]));
}
