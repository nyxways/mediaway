//! Unit tests for the `WebM` facade adapter (sibling of `webm.rs`).

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;
use ebml_webm::TrackInfo as CoreTrackInfo;

fn track(codec_id: &str, is_video: bool, width: u32, height: u32) -> CoreTrackInfo {
    CoreTrackInfo {
        track_number: 1,
        track_type: if is_video { 1 } else { 2 },
        codec_id: codec_id.to_string(),
        width,
        height,
        sample_rate: 8000.0,
        channels: 1,
    }
}

#[test]
fn codec_kind_maps_supported_codecs() {
    assert_eq!(codec_kind("V_VP9"), Some(CodecKind::Vp9));
    assert_eq!(codec_kind("V_AV1"), Some(CodecKind::Av1));
    assert_eq!(codec_kind("A_OPUS"), Some(CodecKind::Opus));
    assert_eq!(codec_kind("A_VORBIS"), Some(CodecKind::Vorbis));
    assert_eq!(codec_kind("A_AAC"), Some(CodecKind::Aac));
    assert_eq!(codec_kind("A_AAC/MPEG4/LC"), Some(CodecKind::Aac));
}

#[test]
fn codec_kind_none_for_unmapped_webm_codecs() {
    // VP8 is a common real WebM video codec with no CodecKind variant yet.
    assert_eq!(codec_kind("V_VP8"), None);
}

#[test]
fn track_id_truncates_out_of_range_track_number() {
    assert_eq!(track_id(1), 1);
    assert_eq!(track_id(u64::from(u32::MAX)), u32::MAX);
    assert_eq!(track_id(u64::from(u32::MAX) + 1), u32::MAX);
}

#[test]
fn to_stream_info_none_for_unsupported_codec() {
    let t = track("V_VP8", true, 1280, 720);
    assert_eq!(to_stream_info(&t, MwRational::new(1, 1000)), None);
}

#[test]
fn to_stream_info_maps_video_geometry() {
    let t = track("V_VP9", true, 1280, 720);
    let info = to_stream_info(&t, MwRational::new(1, 1000)).expect("supported codec");
    assert_eq!(info.id(), 1);
    assert_eq!(info.codec(), CodecKind::Vp9);
    assert_eq!(
        info.geometry(),
        Some(VideoGeometry {
            width: 1280,
            height: 720
        })
    );
}

#[test]
fn to_stream_info_non_video_has_no_geometry() {
    let t = track("A_OPUS", false, 0, 0);
    let info = to_stream_info(&t, MwRational::new(1, 1000)).expect("supported codec");
    assert_eq!(info.geometry(), None);
}

#[test]
fn to_stream_info_threads_real_sample_rate_and_channels() {
    let mut t = track("A_OPUS", false, 0, 0);
    t.sample_rate = 48_000.0;
    t.channels = 2;
    let info = to_stream_info(&t, MwRational::new(1, 1000)).expect("supported codec");
    assert_eq!(info.sample_rate(), Some(48_000));
    assert_eq!(info.channels(), Some(2));
}

#[test]
fn sample_rate_u32_saturates_out_of_range_and_non_finite() {
    assert_eq!(sample_rate_u32(48_000.0), 48_000);
    assert_eq!(sample_rate_u32(-1.0), 0);
    // NaN/+-infinity are malformed EBML Float input either way; treated the
    // same as "unknown" (0), not saturated — see `sample_rate_u32`'s
    // `is_finite` check.
    assert_eq!(sample_rate_u32(f64::NAN), 0);
    assert_eq!(sample_rate_u32(f64::INFINITY), 0);
}

#[test]
fn channels_u16_saturates_out_of_range() {
    assert_eq!(channels_u16(2), 2);
    assert_eq!(channels_u16(0), 0);
    assert_eq!(channels_u16(u32::from(u16::MAX) + 1), u16::MAX);
}

/// Encode an element size VINT (marker stripped), smallest length that fits.
/// Mirrors `ebml_webm::demux_tests` — test-only byte-fixture duplication.
#[allow(
    clippy::cast_possible_truncation,
    reason = "len is bounded to 1..=8 by the loop range, always fits usize"
)]
fn enc_size(value: u64) -> Vec<u8> {
    for len in 1u64..=8 {
        let data_bits = 7 * len;
        let max = (1u64 << data_bits) - 2;
        if value <= max {
            let full = value | (1u64 << data_bits);
            let be = full.to_be_bytes();
            return be[8 - len as usize..].to_vec();
        }
    }
    Vec::new()
}

fn elem(id: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(id);
    out.extend_from_slice(&enc_size(payload.len() as u64));
    out.extend_from_slice(payload);
    out
}

fn uint_payload(v: u64, len: usize) -> Vec<u8> {
    let be = v.to_be_bytes();
    be[8 - len..].to_vec()
}

/// A full `WebM` byte stream: header (skipped) + Segment{Info, Tracks{one
/// video track, `V_VP9`}, Cluster{Timecode, `cluster_child`}}.
fn build_webm_with_cluster_child(cluster_child: &[u8]) -> Vec<u8> {
    let tracks = {
        let num = elem(&[0xD7], &uint_payload(1, 1)); // TrackNumber = 1
        let typ = elem(&[0x83], &uint_payload(1, 1)); // TrackType = video
        let codec = elem(&[0x86], b"V_VP9"); // CodecID
        let mut body = Vec::new();
        body.extend_from_slice(&num);
        body.extend_from_slice(&typ);
        body.extend_from_slice(&codec);
        let track_entry = elem(&[0xAE], &body); // TrackEntry
        elem(&[0x16, 0x54, 0xAE, 0x6B], &track_entry) // Tracks
    };
    let info = elem(&[0x2A, 0xD7, 0xB1], &uint_payload(1_000_000, 3)); // TimecodeScale
    let cluster = {
        let timecode = elem(&[0xE7], &uint_payload(0, 1)); // Timecode = 0
        let mut body = Vec::new();
        body.extend_from_slice(&timecode);
        body.extend_from_slice(cluster_child);
        elem(&[0x1F, 0x43, 0xB6, 0x75], &body) // Cluster
    };
    let mut segment_body = Vec::new();
    segment_body.extend_from_slice(&info);
    segment_body.extend_from_slice(&tracks);
    segment_body.extend_from_slice(&cluster);
    let segment = elem(&[0x18, 0x53, 0x80, 0x67], &segment_body); // Segment
    let header = elem(&[0x1A, 0x45, 0xDF, 0xA3], &[]); // EBML header (empty, skipped)

    let mut out = Vec::new();
    out.extend_from_slice(&header);
    out.extend_from_slice(&segment);
    out
}

#[test]
fn poll_packet_wires_block_group_duration() {
    let block = {
        let mut body = Vec::new();
        body.push(0x81); // track number = 1
        body.extend_from_slice(&0i16.to_be_bytes());
        body.push(0x00); // flags: no lacing
        body.extend_from_slice(&[9, 9, 9]);
        elem(&[0xA1], &body) // Block
    };
    let duration = elem(&[0x9B], &uint_payload(33, 1)); // BlockDuration
    let mut group_body = Vec::new();
    group_body.extend_from_slice(&block);
    group_body.extend_from_slice(&duration);
    let block_group = elem(&[0xA0], &group_body); // BlockGroup

    let bytes = build_webm_with_cluster_child(&block_group);
    let mut d = Demuxer::new();
    d.push_bytes(&bytes);

    let packet = d.poll_packet().expect("BlockGroup packet");
    assert_eq!(&packet.payload[..], &[9, 9, 9]);
    assert_eq!(packet.duration, 33);
}

#[test]
fn cues_and_seek_head_pass_through_from_core() {
    let cue_point = {
        let time = elem(&[0xB3], &uint_payload(5, 1)); // CueTime
        let track_positions = {
            let track = elem(&[0xF7], &uint_payload(1, 1)); // CueTrack
            let pos = elem(&[0xF1], &uint_payload(1234, 2)); // CueClusterPosition
            let mut body = Vec::new();
            body.extend_from_slice(&track);
            body.extend_from_slice(&pos);
            elem(&[0xB7], &body) // CueTrackPositions
        };
        let mut body = Vec::new();
        body.extend_from_slice(&time);
        body.extend_from_slice(&track_positions);
        elem(&[0xBB], &body) // CuePoint
    };
    let cues = elem(&[0x1C, 0x53, 0xBB, 0x6B], &cue_point); // Cues

    let seek = {
        let seek_id = elem(&[0x53, 0xAB], &[0x16, 0x54, 0xAE, 0x6B]); // SeekID -> Tracks
        let seek_position = elem(&[0x53, 0xAC], &uint_payload(42, 1)); // SeekPosition
        let mut body = Vec::new();
        body.extend_from_slice(&seek_id);
        body.extend_from_slice(&seek_position);
        elem(&[0x4D, 0xBB], &body) // Seek
    };
    let seek_head = elem(&[0x11, 0x4D, 0x9B, 0x74], &seek); // SeekHead

    let mut segment_body = Vec::new();
    segment_body.extend_from_slice(&seek_head);
    segment_body.extend_from_slice(&cues);
    let segment = elem(&[0x18, 0x53, 0x80, 0x67], &segment_body); // Segment
    let header = elem(&[0x1A, 0x45, 0xDF, 0xA3], &[]); // EBML header (empty, skipped)
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(&segment);

    let mut d = Demuxer::new();
    d.push_bytes(&bytes);

    assert_eq!(d.cues().len(), 1);
    assert_eq!(d.cues()[0].time_ticks, 5);
    assert_eq!(d.cues()[0].cluster_position, 1234);

    assert_eq!(d.seek_head().len(), 1);
    assert_eq!(d.seek_head()[0].id, ebml_webm::ids::TRACKS);
    assert_eq!(d.seek_head()[0].position, 42);
}

#[cfg(feature = "mux")]
fn video_stream(id: u32) -> StreamInfo {
    StreamInfo::Video {
        id,
        codec: CodecKind::Vp9,
        time_base: MwRational::new(1, 1000),
        geometry: VideoGeometry {
            width: 640,
            height: 480,
        },
        extra_data: Bytes::new(),
    }
}

#[cfg(feature = "mux")]
fn audio_stream(id: u32) -> StreamInfo {
    StreamInfo::Audio {
        id,
        codec: CodecKind::Opus,
        time_base: MwRational::new(1, 1000),
        extra_data: Bytes::new(),
        sample_rate: 48_000,
        channels: 2,
    }
}

#[cfg(feature = "mux")]
#[test]
fn add_track_rejects_unsupported_codec() {
    let mut m = Muxer::<Open>::new();
    let unsupported = StreamInfo::Video {
        id: 0,
        codec: CodecKind::H264,
        time_base: MwRational::new(1, 1000),
        geometry: VideoGeometry {
            width: 640,
            height: 480,
        },
        extra_data: Bytes::new(),
    };
    assert_eq!(
        m.add_track(unsupported),
        Err(Error::UnsupportedCodec(CodecKind::H264))
    );
}

#[cfg(feature = "mux")]
#[test]
fn add_track_rejects_stream_id_zero() {
    // WebM TrackNumber 0 is reserved (ebml-webm's own MuxError::InvalidTrackNumber);
    // this facade maps StreamInfo::id directly to TrackNumber, so id 0 is invalid too.
    let mut m = Muxer::<Open>::new();
    assert_eq!(
        m.add_track(video_stream(0)),
        Err(Error::Mux(ebml_webm::MuxError::InvalidTrackNumber))
    );
}

#[cfg(feature = "mux")]
#[test]
fn push_packet_rejects_unregistered_stream() {
    let mut m = Muxer::<Open>::new();
    m.add_track(video_stream(1)).unwrap();
    let mut live = m.begin();
    let packet = Packet {
        stream_id: 99,
        pts: 0,
        dts: 0,
        duration: 0,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::from_static(b"x"),
    };
    assert_eq!(live.push_packet(&packet), Err(Error::UnknownStream(99)));
}

#[cfg(feature = "mux")]
#[test]
fn mux_video_audio_round_trips_through_demuxer() {
    let mut m = Muxer::<Open>::new();
    let v_id = m.add_track(video_stream(1)).unwrap();
    let a_id = m.add_track(audio_stream(2)).unwrap();
    let mut live = m.begin();

    let mk = |stream_id: u32, pts: i64, keyframe: bool, data: &'static [u8]| Packet {
        stream_id,
        pts,
        dts: pts,
        duration: 0,
        is_keyframe: keyframe,
        is_discard: false,
        payload: Bytes::from_static(data),
    };

    live.push_packet(&mk(v_id, 0, true, b"vframe0")).unwrap();
    live.push_packet(&mk(a_id, 0, true, b"aframe0")).unwrap();
    live.push_packet(&mk(v_id, 33, false, b"vframe1")).unwrap();
    live.flush();

    let mut bytes = Vec::new();
    live.poll_bytes(&mut bytes);
    assert!(!bytes.is_empty());

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);

    let streams = demux.streams();
    assert_eq!(streams.len(), 2);
    assert!(streams.iter().any(|s| s.codec() == CodecKind::Vp9));
    assert!(streams.iter().any(|s| s.codec() == CodecKind::Opus));

    let mut packets = Vec::new();
    while let Some(p) = demux.poll_packet() {
        packets.push(p);
    }
    assert_eq!(packets.len(), 3);
    assert!(
        packets
            .iter()
            .any(|p| p.stream_id == v_id && &p.payload[..] == b"vframe0" && p.is_keyframe)
    );
    assert!(
        packets
            .iter()
            .any(|p| p.stream_id == a_id && &p.payload[..] == b"aframe0")
    );
    assert!(
        packets
            .iter()
            .any(|p| p.stream_id == v_id && p.pts == 33 && &p.payload[..] == b"vframe1")
    );
}

#[test]
fn poll_packet_defaults_duration_for_simple_block() {
    let simple_block = {
        let mut body = Vec::new();
        body.push(0x81); // track number = 1
        body.extend_from_slice(&0i16.to_be_bytes());
        body.push(0x80); // keyframe, no lacing
        body.extend_from_slice(&[1, 2, 3]);
        elem(&[0xA3], &body) // SimpleBlock
    };

    let bytes = build_webm_with_cluster_child(&simple_block);
    let mut d = Demuxer::new();
    d.push_bytes(&bytes);

    let packet = d.poll_packet().expect("SimpleBlock packet");
    assert_eq!(packet.duration, 0);
}
