//! Unit tests for the FLV facade adapter (sibling of `flv.rs`).

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

fn avc_seq_header_tag(ts: u32, avcc: &[u8]) -> Tag {
    let mut data = vec![0x17, 0, 0, 0, 0]; // FrameType=1(key) CodecID=7, AVCPacketType=0, CT=0
    data.extend_from_slice(avcc);
    Tag {
        tag_type: TagType::Video,
        timestamp_ms: ts,
        data: Bytes::copy_from_slice(&data),
    }
}

fn avc_nalu_tag(ts: u32, keyframe: bool, composition_time_ms: i32, nalu: &[u8]) -> Tag {
    let frame_type = if keyframe { 1 } else { 2 };
    let ct = composition_time_ms.to_be_bytes();
    let mut data = vec![(frame_type << 4) | 7, 1, ct[1], ct[2], ct[3]];
    data.extend_from_slice(nalu);
    Tag {
        tag_type: TagType::Video,
        timestamp_ms: ts,
        data: Bytes::copy_from_slice(&data),
    }
}

fn aac_seq_header_tag(ts: u32, asc: &[u8]) -> Tag {
    let mut data = vec![0xAF, 0]; // SoundFormat=10(AAC), rate/size/type bits, AACPacketType=0
    data.extend_from_slice(asc);
    Tag {
        tag_type: TagType::Audio,
        timestamp_ms: ts,
        data: Bytes::copy_from_slice(&data),
    }
}

fn aac_raw_tag(ts: u32, raw: &[u8]) -> Tag {
    let mut data = vec![0xAF, 1];
    data.extend_from_slice(raw);
    Tag {
        tag_type: TagType::Audio,
        timestamp_ms: ts,
        data: Bytes::copy_from_slice(&data),
    }
}

fn mp3_tag(ts: u32, frame: &[u8]) -> Tag {
    let mut data = vec![0x2F]; // SoundFormat=2(MP3)
    data.extend_from_slice(frame);
    Tag {
        tag_type: TagType::Audio,
        timestamp_ms: ts,
        data: Bytes::copy_from_slice(&data),
    }
}

fn build_flv(tags: &[Tag]) -> Vec<u8> {
    let mut mux = flv_core::Muxer::new();
    let mut out = Vec::new();
    mux.write_header(true, true, &mut out);
    for tag in tags {
        mux.write_tag(tag, &mut out).expect("write tag");
    }
    out
}

#[test]
fn avc_stream_roundtrips_extradata_and_composition_time() {
    let avcc = [1, 0x42, 0, 0x1e, 0xff, 0xe1, 0, 0];
    let bytes = build_flv(&[
        avc_seq_header_tag(0, &avcc),
        avc_nalu_tag(33, true, 12, &[0, 0, 0, 2, 0x65, 0x88]),
        avc_nalu_tag(66, false, 0, &[0, 0, 0, 2, 0x41, 0x99]),
    ]);

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);

    let p1 = demux.poll_packet().expect("first NALU packet");
    assert_eq!(p1.stream_id, VIDEO_STREAM_ID);
    assert_eq!(p1.dts, 33);
    assert_eq!(p1.pts, 33 + 12);
    assert!(p1.is_keyframe);
    assert_eq!(&p1.payload[..], &[0, 0, 0, 2, 0x65, 0x88]);

    let p2 = demux.poll_packet().expect("second NALU packet");
    assert!(!p2.is_keyframe);

    let streams = demux.streams();
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].codec(), CodecKind::H264);
    assert_eq!(streams[0].extra_data().as_ref(), &avcc[..]);
}

#[test]
fn aac_stream_roundtrips_extradata() {
    let asc = [0x12, 0x10];
    let bytes = build_flv(&[aac_seq_header_tag(0, &asc), aac_raw_tag(23, &[1, 2, 3, 4])]);

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);

    let p = demux.poll_packet().expect("aac frame");
    assert_eq!(p.stream_id, AUDIO_STREAM_ID);
    assert_eq!(p.pts, 23);
    assert_eq!(&p.payload[..], &[1, 2, 3, 4]);

    let streams = demux.streams();
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].codec(), CodecKind::Aac);
    assert_eq!(streams[0].extra_data().as_ref(), &asc[..]);
}

#[test]
fn mp3_stream_maps_to_mp3_codec() {
    let bytes = build_flv(&[mp3_tag(0, &[0xFF, 0xFB, 0x90, 0])]);

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);

    let p = demux.poll_packet().expect("mp3 frame");
    assert_eq!(&p.payload[..], &[0xFF, 0xFB, 0x90, 0]);
    assert_eq!(demux.streams()[0].codec(), CodecKind::Mp3);
}

fn video_track(extra_data: &[u8]) -> StreamInfo {
    StreamInfo::Video {
        id: VIDEO_STREAM_ID,
        codec: CodecKind::H264,
        time_base: MS_TIME_BASE,
        geometry: mediaway_common::VideoGeometry {
            width: 1280,
            height: 720,
        },
        extra_data: Bytes::copy_from_slice(extra_data),
    }
}

fn aac_track(extra_data: &[u8]) -> StreamInfo {
    StreamInfo::Audio {
        id: AUDIO_STREAM_ID,
        codec: CodecKind::Aac,
        time_base: MS_TIME_BASE,
        extra_data: Bytes::copy_from_slice(extra_data),
        sample_rate: 44_100,
        channels: 2,
    }
}

#[test]
fn push_packet_writes_sequence_header_before_first_data_tag() {
    let avcc = [1, 0x42, 0, 0x1e];
    let mut mux = Muxer::new();
    mux.add_track(&video_track(&avcc)).expect("add video track");

    let mut out = Vec::new();
    mux.write_header(false, true, &mut out);
    mux.push_packet(
        &Packet {
            stream_id: VIDEO_STREAM_ID,
            pts: 10,
            dts: 10,
            duration: 0,
            is_keyframe: true,
            is_discard: false,
            payload: Bytes::copy_from_slice(&[0, 0, 0, 1, 0x65]),
        },
        &mut out,
    )
    .expect("push first packet");
    mux.push_packet(
        &Packet {
            stream_id: VIDEO_STREAM_ID,
            pts: 43,
            dts: 43,
            duration: 0,
            is_keyframe: false,
            is_discard: false,
            payload: Bytes::copy_from_slice(&[0, 0, 0, 1, 0x41]),
        },
        &mut out,
    )
    .expect("push second packet");

    // Read raw tags via the core demuxer to assert exact ordering/count: the
    // sequence header (AVCPacketType=0) is written once, before any data tag
    // (AVCPacketType=1) — not re-emitted on the second `push_packet` call.
    let mut core_demux = flv_core::Demuxer::new();
    core_demux.push_bytes(&out);
    let tag1 = core_demux.poll_tag().expect("poll").expect("tag1");
    assert_eq!(tag1.data[1], 0); // AVCPacketType = 0 (sequence header)
    assert_eq!(&tag1.data[5..], &avcc[..]);
    let tag2 = core_demux.poll_tag().expect("poll").expect("tag2");
    assert_eq!(tag2.data[1], 1); // AVCPacketType = 1 (NALU)
    let tag3 = core_demux.poll_tag().expect("poll").expect("tag3");
    assert_eq!(tag3.data[1], 1);
    assert!(core_demux.poll_tag().expect("poll").is_none());
}

#[test]
fn mux_then_demux_roundtrips_avc_and_aac_packets() {
    let avcc = [1, 0x42, 0, 0x1e, 0xff, 0xe1, 0, 0];
    let asc = [0x12, 0x10];

    let mut mux = Muxer::new();
    mux.add_track(&video_track(&avcc)).expect("add video track");
    mux.add_track(&aac_track(&asc)).expect("add audio track");

    let video_packets = [
        Packet {
            stream_id: VIDEO_STREAM_ID,
            pts: 45,
            dts: 33,
            duration: 0,
            is_keyframe: true,
            is_discard: false,
            payload: Bytes::copy_from_slice(&[0, 0, 0, 2, 0x65, 0x88]),
        },
        Packet {
            stream_id: VIDEO_STREAM_ID,
            pts: 66,
            dts: 66,
            duration: 0,
            is_keyframe: false,
            is_discard: false,
            payload: Bytes::copy_from_slice(&[0, 0, 0, 2, 0x41, 0x99]),
        },
    ];
    let audio_packets = [Packet {
        stream_id: AUDIO_STREAM_ID,
        pts: 23,
        dts: 23,
        duration: 0,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::copy_from_slice(&[1, 2, 3, 4]),
    }];

    let mut out = Vec::new();
    mux.write_header(true, true, &mut out);
    for p in &video_packets {
        mux.push_packet(p, &mut out).expect("push video packet");
    }
    for p in &audio_packets {
        mux.push_packet(p, &mut out).expect("push audio packet");
    }

    let mut demux = Demuxer::new();
    demux.push_bytes(&out);

    let mut got_video = Vec::new();
    let mut got_audio = Vec::new();
    while let Some(p) = demux.poll_packet() {
        if p.stream_id == VIDEO_STREAM_ID {
            got_video.push(p);
        } else {
            got_audio.push(p);
        }
    }

    assert_eq!(got_video, video_packets);
    assert_eq!(got_audio, audio_packets);

    let streams = demux.streams();
    let video_stream = streams
        .iter()
        .find(|s| s.id() == VIDEO_STREAM_ID)
        .expect("video stream");
    assert_eq!(video_stream.codec(), CodecKind::H264);
    assert_eq!(video_stream.extra_data().as_ref(), &avcc[..]);
    let audio_stream = streams
        .iter()
        .find(|s| s.id() == AUDIO_STREAM_ID)
        .expect("audio stream");
    assert_eq!(audio_stream.codec(), CodecKind::Aac);
    assert_eq!(audio_stream.extra_data().as_ref(), &asc[..]);
}

#[test]
fn mux_then_demux_roundtrips_mp3_packet() {
    let mut mux = Muxer::new();
    mux.add_track(&StreamInfo::Audio {
        id: AUDIO_STREAM_ID,
        codec: CodecKind::Mp3,
        time_base: MS_TIME_BASE,
        extra_data: Bytes::new(),
        sample_rate: 44_100,
        channels: 2,
    })
    .expect("add audio track");

    let packet = Packet {
        stream_id: AUDIO_STREAM_ID,
        pts: 12,
        dts: 12,
        duration: 0,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::copy_from_slice(&[0xFF, 0xFB, 0x90, 0]),
    };

    let mut out = Vec::new();
    mux.write_header(true, false, &mut out);
    mux.push_packet(&packet, &mut out).expect("push mp3 packet");

    let mut demux = Demuxer::new();
    demux.push_bytes(&out);
    let got = demux.poll_packet().expect("mp3 packet");
    assert_eq!(got, packet);
    assert_eq!(demux.streams()[0].codec(), CodecKind::Mp3);
}

#[test]
fn add_track_rejects_unsupported_codec() {
    let mut mux = Muxer::new();
    let stream = StreamInfo::Video {
        id: VIDEO_STREAM_ID,
        codec: CodecKind::Hevc,
        time_base: MS_TIME_BASE,
        geometry: mediaway_common::VideoGeometry {
            width: 0,
            height: 0,
        },
        extra_data: Bytes::new(),
    };
    assert!(matches!(
        mux.add_track(&stream),
        Err(Error::UnsupportedCodec(CodecKind::Hevc))
    ));
}

#[test]
fn push_packet_rejects_unregistered_stream() {
    let mut mux = Muxer::new();
    let mut out = Vec::new();
    mux.write_header(true, true, &mut out);
    let packet = Packet {
        stream_id: VIDEO_STREAM_ID,
        pts: 0,
        dts: 0,
        duration: 0,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::copy_from_slice(&[0, 0, 0, 1, 0x65]),
    };
    assert!(matches!(
        mux.push_packet(&packet, &mut out),
        Err(Error::UnregisteredStream(VIDEO_STREAM_ID))
    ));
}

#[test]
fn unrecognized_video_codec_id_is_dropped() {
    // CodecID = 4 (VP6) — no CodecKind mapping.
    let tag = Tag {
        tag_type: TagType::Video,
        timestamp_ms: 0,
        data: Bytes::from_static(&[0x14, 0, 0, 0]),
    };
    let bytes = build_flv(&[tag]);

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    assert!(demux.poll_packet().is_none());
    assert!(demux.streams().is_empty());
}
