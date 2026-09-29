//! Integration: a replay ring over REAL H.264 packets, entirely through `mediaway-ffi`'s C ABI
//! (`adr/container/0009-replay-ring-c-abi.md`).
//!
//! Encodes with the WMF H.264 encoder, demuxes the fMP4 back into packets (the only packet
//! source a C caller has for video), and then:
//!
//! 1. pushes them into a `BYTES` ring, cuts a clip, muxes it with a fresh muxer, demuxes that,
//!    and checks the payloads and the rebased timestamps;
//! 2. muxes the same packets with a placements muxer and checks every placement points at the
//!    real bytes in the "file", and that placements do not change the output;
//! 3. feeds a `STORED` ring from those placements and checks it cuts the same clip, whose bytes
//!    read back from the file equal the `BYTES` clip's.
//!
//! One WMF encode session emits a single keyframe and then P frames, so the stream is built from
//! four sessions of 30 frames laid end to end: four real GOPs, cut points at frames 0, 30, 60 and
//! 90. The reordering logic is covered by `src/container/replay_tests.rs` and the Rust ring's own
//! tests.

#![cfg(all(windows, feature = "pipeline"))]
#![allow(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::too_many_lines,
    clippy::missing_const_for_fn,
    reason = "integration test"
)]

use mediaway_ffi::container::{
    MediawayCodecKind, MediawayPacket, MediawayPacketMeta, MediawayPacketView, MediawayPlacement,
    MediawayRational, MediawayReplayClipEntry, MediawayReplayPayloadKind, MediawayReplayRingConfig,
    MediawayStatus, MediawayStoredPayload, MediawayStreamInfo, MediawayVideoTrackInfo,
    mediaway_buffer_free, mediaway_demuxer_close, mediaway_demuxer_create,
    mediaway_demuxer_poll_packet, mediaway_demuxer_push_bytes, mediaway_demuxer_stream_at,
    mediaway_muxer_add_video_track, mediaway_muxer_begin, mediaway_muxer_close,
    mediaway_muxer_create, mediaway_muxer_create_with_placements, mediaway_muxer_flush,
    mediaway_muxer_poll_bytes, mediaway_muxer_poll_placements, mediaway_muxer_push_packet,
    mediaway_packet_free, mediaway_placements_free, mediaway_replay_clip_free,
    mediaway_replay_clip_packet_at, mediaway_replay_clip_packet_count,
    mediaway_replay_ring_clip_last, mediaway_replay_ring_close, mediaway_replay_ring_create,
    mediaway_replay_ring_push, mediaway_replay_ring_push_stored, mediaway_stream_info_free,
};
use mediaway_ffi::pipeline::{
    MediawayGpuBufferHandle, MediawayGpuBufferKind, MediawayPipelineCodecKind,
    MediawayPipelineStatus, MediawayPixelFormat, MediawayVideoFrame, MediawayVideoFrameStorageKind,
    mediaway_auto_encoder_open, mediaway_auto_video_encode_config_new,
    mediaway_encode_session_finish, mediaway_encode_session_open,
    mediaway_encode_session_write_frame, mediaway_pipeline_ffi_buffer_free,
};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;
/// Frames per encode session, i.e. per GOP.
const GOP_FRAMES: u32 = 30;
const GOPS: u32 = 4;
const FRAMES: u32 = GOP_FRAMES * GOPS;

struct Owned {
    pts: i64,
    dts: i64,
    duration: u64,
    is_keyframe: bool,
    payload: Vec<u8>,
}

struct Stream {
    time_base: MediawayRational,
    extra_data: Vec<u8>,
}

/// Encode `GOP_FRAMES` distinct frames and return the whole fMP4, or `None` with no encoder.
fn encode_one_gop() -> Option<Vec<u8>> {
    let config = mediaway_auto_video_encode_config_new(
        MediawayPipelineCodecKind::H264,
        WIDTH,
        HEIGHT,
        mediaway_ffi::pipeline::MediawayRational { num: 1, den: 30 },
    );
    let mut encoder = std::ptr::null_mut();
    if unsafe { mediaway_auto_encoder_open(&raw const config, &raw mut encoder) }
        == MediawayPipelineStatus::NoBackend
    {
        eprintln!("skip: no encode backend compiled in");
        return None;
    }
    let mut session = std::ptr::null_mut();
    assert_eq!(
        unsafe { mediaway_encode_session_open(encoder, &raw mut session) },
        MediawayPipelineStatus::Ok
    );
    for i in 0..GOP_FRAMES {
        // A different gray per frame, so each packet's bytes are its own.
        let plane = vec![((i * 3) % 200 + 20) as u8; (WIDTH * HEIGHT * 3 / 2) as usize];
        let frame = MediawayVideoFrame {
            pts: i64::from(i),
            duration: 1,
            width: WIDTH,
            height: HEIGHT,
            pixel_format: MediawayPixelFormat::Nv12,
            storage_kind: MediawayVideoFrameStorageKind::Cpu,
            raw_bytes: plane.as_ptr(),
            raw_bytes_len: plane.len(),
            gpu_buffer: MediawayGpuBufferHandle {
                kind: MediawayGpuBufferKind::DirectX11,
                native_a: 0,
                native_b: 0,
                subresource: 0,
                webgpu_texture_id: 0,
            },
        };
        assert_eq!(
            unsafe { mediaway_encode_session_write_frame(session, &raw const frame) },
            MediawayPipelineStatus::Ok
        );
    }
    let (mut data, mut len) = (std::ptr::null_mut(), 0usize);
    assert_eq!(
        unsafe { mediaway_encode_session_finish(session, &raw mut data, &raw mut len) },
        MediawayPipelineStatus::Ok
    );
    let bytes = unsafe { std::slice::from_raw_parts(data, len) }.to_vec();
    unsafe { mediaway_pipeline_ffi_buffer_free(data, len) };
    Some(bytes)
}

/// Demux an fMP4 into its stream info and packets, in file order.
fn demux(fmp4: &[u8]) -> (Stream, Vec<Owned>) {
    let demuxer = mediaway_demuxer_create();
    assert_eq!(
        unsafe { mediaway_demuxer_push_bytes(demuxer, fmp4.as_ptr(), fmp4.len()) },
        MediawayStatus::Ok
    );
    let mut info = MediawayStreamInfo {
        id: 0,
        codec: MediawayCodecKind::H264,
        time_base: MediawayRational { num: 0, den: 1 },
        has_geometry: false,
        width: 0,
        height: 0,
        sample_rate: 0,
        channels: 0,
        extra_data: std::ptr::null_mut(),
        extra_data_len: 0,
    };
    assert_eq!(
        unsafe { mediaway_demuxer_stream_at(demuxer, 0, &raw mut info) },
        MediawayStatus::Ok
    );
    // SAFETY: valid for the lifetime of `info`.
    let extra_data =
        unsafe { std::slice::from_raw_parts(info.extra_data, info.extra_data_len) }.to_vec();
    let stream = Stream {
        time_base: info.time_base,
        extra_data,
    };
    unsafe { mediaway_stream_info_free(&raw mut info) };

    let mut packets = Vec::new();
    loop {
        let mut packet = MediawayPacket {
            stream_id: 0,
            pts: 0,
            dts: 0,
            duration: 0,
            is_keyframe: false,
            is_discard: false,
            payload: std::ptr::null_mut(),
            payload_len: 0,
        };
        let mut has = false;
        assert_eq!(
            unsafe { mediaway_demuxer_poll_packet(demuxer, &raw mut packet, &raw mut has) },
            MediawayStatus::Ok
        );
        if !has {
            break;
        }
        packets.push(Owned {
            pts: packet.pts,
            dts: packet.dts,
            duration: packet.duration,
            is_keyframe: packet.is_keyframe,
            // SAFETY: valid for the lifetime of `packet`.
            payload: unsafe { std::slice::from_raw_parts(packet.payload, packet.payload_len) }
                .to_vec(),
        });
        unsafe { mediaway_packet_free(&raw mut packet) };
    }
    unsafe { mediaway_demuxer_close(demuxer) };
    (stream, packets)
}

/// `GOPS` encode sessions laid end to end on one timeline: four keyframes, each starting a
/// real GOP. `None` when the machine has no encoder.
fn multi_gop_stream() -> Option<(Stream, Vec<Owned>)> {
    let mut stream = None;
    let mut all: Vec<Owned> = Vec::new();
    for _ in 0..GOPS {
        let fmp4 = encode_one_gop()?;
        let (s, mut packets) = demux(&fmp4);
        // Continue the timeline where the last session ended.
        let base = all
            .last()
            .map_or(0, |p| p.dts + i64::try_from(p.duration).unwrap());
        let first = packets[0].dts;
        for p in &mut packets {
            p.pts = p.pts - first + base;
            p.dts = p.dts - first + base;
        }
        stream.get_or_insert(s);
        all.extend(packets);
    }
    assert_eq!(all.len(), FRAMES as usize);
    assert_eq!(
        all.iter().filter(|p| p.is_keyframe).count(),
        GOPS as usize,
        "one keyframe per GOP"
    );
    Some((stream.unwrap(), all))
}

fn view(p: &Owned) -> MediawayPacketView {
    MediawayPacketView {
        stream_id: 0,
        pts: p.pts,
        dts: p.dts,
        duration: p.duration,
        is_keyframe: p.is_keyframe,
        is_discard: false,
        payload: p.payload.as_ptr(),
        payload_len: p.payload.len(),
    }
}

/// Mux `packets` with a muxer from `create`, returning every output byte (and the muxer's
/// placements when it records them).
fn mux(
    create: extern "C" fn() -> *mut mediaway_ffi::container::MuxerHandle,
    stream: &Stream,
    packets: &[Owned],
    take_placements: bool,
) -> (Vec<u8>, Vec<MediawayPlacement>) {
    let muxer = create();
    let track = MediawayVideoTrackInfo {
        id: 0,
        codec: MediawayCodecKind::H264,
        time_base: stream.time_base,
        width: WIDTH,
        height: HEIGHT,
        extra_data: stream.extra_data.as_ptr(),
        extra_data_len: stream.extra_data.len(),
    };
    assert_eq!(
        unsafe { mediaway_muxer_add_video_track(muxer, &raw const track) },
        MediawayStatus::Ok
    );
    assert_eq!(unsafe { mediaway_muxer_begin(muxer) }, MediawayStatus::Ok);
    let mut file = Vec::new();
    let mut placements = Vec::new();
    for p in packets {
        let v = view(p);
        assert_eq!(
            unsafe { mediaway_muxer_push_packet(muxer, &raw const v) },
            MediawayStatus::Ok
        );
    }
    assert_eq!(unsafe { mediaway_muxer_flush(muxer) }, MediawayStatus::Ok);
    let (mut data, mut len) = (std::ptr::null_mut(), 0usize);
    assert_eq!(
        unsafe { mediaway_muxer_poll_bytes(muxer, &raw mut data, &raw mut len) },
        MediawayStatus::Ok
    );
    if len > 0 {
        file.extend_from_slice(unsafe { std::slice::from_raw_parts(data, len) });
        unsafe { mediaway_buffer_free(data, len) };
    }
    if take_placements {
        let (mut rows, mut count) = (std::ptr::null_mut(), 0usize);
        assert_eq!(
            unsafe { mediaway_muxer_poll_placements(muxer, &raw mut rows, &raw mut count) },
            MediawayStatus::Ok
        );
        if count > 0 {
            placements.extend_from_slice(unsafe { std::slice::from_raw_parts(rows, count) });
            unsafe { mediaway_placements_free(rows, count) };
        }
    }
    unsafe { mediaway_muxer_close(muxer) };
    (file, placements)
}

fn ring(
    kind: MediawayReplayPayloadKind,
    stream: &Stream,
) -> *mut mediaway_ffi::container::ReplayRingHandle {
    let config = MediawayReplayRingConfig {
        anchor_stream_id: 0,
        anchor_time_base: stream.time_base,
        window_ms: 1_500,
        max_bytes: 0,
        payload_kind: kind,
    };
    let mut ring = std::ptr::null_mut();
    assert_eq!(
        unsafe { mediaway_replay_ring_create(&raw const config, &raw mut ring) },
        MediawayStatus::Ok
    );
    ring
}

fn clip_entries(
    ring: *mut mediaway_ffi::container::ReplayRingHandle,
) -> Vec<MediawayReplayClipEntry> {
    let (mut clip, mut has) = (std::ptr::null_mut(), false);
    assert_eq!(
        unsafe { mediaway_replay_ring_clip_last(ring, 1_000, &raw mut clip, &raw mut has) },
        MediawayStatus::Ok
    );
    assert!(has, "the ring holds keyframes, so it must cut a clip");
    let n = unsafe { mediaway_replay_clip_packet_count(clip) };
    let out = (0..n)
        .map(|i| {
            let mut e = unsafe { std::mem::zeroed::<MediawayReplayClipEntry>() };
            assert_eq!(
                unsafe { mediaway_replay_clip_packet_at(clip, i, &raw mut e) },
                MediawayStatus::Ok
            );
            // Own the borrowed payload before the clip is freed.
            if e.payload_kind == MediawayReplayPayloadKind::Bytes {
                let owned =
                    unsafe { std::slice::from_raw_parts(e.payload, e.payload_len) }.to_vec();
                e.payload = Box::leak(owned.into_boxed_slice()).as_ptr();
            }
            e
        })
        .collect();
    unsafe { mediaway_replay_clip_free(clip) };
    out
}

#[test]
fn a_clip_of_real_h264_muxes_to_a_standalone_file() {
    let Some((stream, packets)) = multi_gop_stream() else {
        return;
    };
    assert_eq!(packets.len(), FRAMES as usize);

    let ring = ring(MediawayReplayPayloadKind::Bytes, &stream);
    for p in &packets {
        let v = view(p);
        assert_eq!(
            unsafe { mediaway_replay_ring_push(ring, &raw const v) },
            MediawayStatus::Ok
        );
    }
    let list = clip_entries(ring);
    // Newest is frame 119 (3.97 s). A 1 s clip needs a keyframe at or before 2.97 s, which is the
    // one at frame 60 (2.0 s): whole GOPs, starting up to one GOP earlier than asked.
    assert_eq!(
        list.len(),
        60,
        "a 1 s clip must start at the keyframe of frame 60"
    );
    assert!(list[0].is_keyframe);
    assert_eq!(list[0].dts, 0, "the cut keyframe decodes at zero");

    // The clip is the tail of the stream, byte for byte.
    let tail = &packets[packets.len() - list.len()..];
    for (e, p) in list.iter().zip(tail) {
        let bytes = unsafe { std::slice::from_raw_parts(e.payload, e.payload_len) };
        assert_eq!(
            bytes, p.payload,
            "clip payload differs from the pushed packet"
        );
    }

    // Written out by a fresh muxer, it demuxes as a standalone file of exactly the clip.
    let clip_packets: Vec<Owned> = list
        .iter()
        .map(|e| Owned {
            pts: e.pts,
            dts: e.dts,
            duration: e.duration,
            is_keyframe: e.is_keyframe,
            payload: unsafe { std::slice::from_raw_parts(e.payload, e.payload_len) }.to_vec(),
        })
        .collect();
    let (file, _) = mux(mediaway_muxer_create, &stream, &clip_packets, false);
    let (_, back) = demux(&file);
    assert_eq!(
        back.len(),
        list.len(),
        "the clip file must hold every clip packet"
    );
    assert!(back[0].is_keyframe);
    for (b, p) in back.iter().zip(&clip_packets) {
        assert_eq!(b.payload, p.payload);
    }
    unsafe { mediaway_replay_ring_close(ring) };
}

#[test]
fn placements_point_at_the_real_bytes_and_change_nothing() {
    let Some((stream, packets)) = multi_gop_stream() else {
        return;
    };

    let (plain, none) = mux(mediaway_muxer_create, &stream, &packets, false);
    let (file, placements) = mux(
        mediaway_muxer_create_with_placements,
        &stream,
        &packets,
        true,
    );
    assert!(none.is_empty());
    assert_eq!(
        file, plain,
        "recording placements must not change the output bytes"
    );

    assert_eq!(placements.len(), packets.len(), "one placement per sample");
    for (pl, p) in placements.iter().zip(&packets) {
        assert_eq!(pl.track_id, 0);
        assert_eq!(pl.dts, p.dts);
        let at = usize::try_from(pl.offset).unwrap();
        // The demuxed payload is already length-prefixed, so it is written unchanged.
        assert_eq!(&file[at..at + pl.len as usize], p.payload.as_slice());
    }

    // An Open muxer has not started writing, so there is nothing to take yet: INVALID_STATE.
    let plain_muxer = mediaway_muxer_create();
    let (mut rows, mut count) = (std::ptr::null_mut(), 0usize);
    let status =
        unsafe { mediaway_muxer_poll_placements(plain_muxer, &raw mut rows, &raw mut count) };
    assert_eq!(
        status,
        MediawayStatus::InvalidState,
        "an Open muxer has no placements to take"
    );
    unsafe { mediaway_muxer_close(plain_muxer) };
}

#[test]
fn a_stored_ring_cuts_the_same_clip_and_its_locations_read_back_the_same_bytes() {
    let Some((stream, packets)) = multi_gop_stream() else {
        return;
    };
    let (file, placements) = mux(
        mediaway_muxer_create_with_placements,
        &stream,
        &packets,
        true,
    );

    let bytes_ring = ring(MediawayReplayPayloadKind::Bytes, &stream);
    let stored_ring = ring(MediawayReplayPayloadKind::Stored, &stream);
    for (p, pl) in packets.iter().zip(&placements) {
        let v = view(p);
        assert_eq!(
            unsafe { mediaway_replay_ring_push(bytes_ring, &raw const v) },
            MediawayStatus::Ok
        );
        let meta = MediawayPacketMeta {
            stream_id: 0,
            pts: p.pts,
            dts: p.dts,
            duration: p.duration,
            is_keyframe: p.is_keyframe,
            is_discard: false,
        };
        let stored = MediawayStoredPayload {
            file: 1,
            offset: pl.offset,
            len: pl.len,
        };
        assert_eq!(
            unsafe {
                mediaway_replay_ring_push_stored(stored_ring, &raw const meta, &raw const stored)
            },
            MediawayStatus::Ok
        );
    }

    let from_bytes = clip_entries(bytes_ring);
    let from_stored = clip_entries(stored_ring);
    assert_eq!(
        from_bytes.len(),
        from_stored.len(),
        "both rings cut the same clip"
    );
    for (b, s) in from_bytes.iter().zip(&from_stored) {
        assert_eq!(
            (b.pts, b.dts, b.duration, b.is_keyframe),
            (s.pts, s.dts, s.duration, s.is_keyframe)
        );
        assert_eq!(s.payload_kind, MediawayReplayPayloadKind::Stored);
        assert_eq!(s.stored_file, 1);
        let at = usize::try_from(s.stored_offset).unwrap();
        let read_back = &file[at..at + s.stored_len as usize];
        assert_eq!(
            read_back,
            unsafe { std::slice::from_raw_parts(b.payload, b.payload_len) },
            "the stored location must read back the bytes the BYTES ring holds"
        );
    }
    unsafe {
        mediaway_replay_ring_close(bytes_ring);
        mediaway_replay_ring_close(stored_ring);
    }
}
