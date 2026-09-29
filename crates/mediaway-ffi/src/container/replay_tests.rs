//! Hermetic tests for the replay ring C ABI (`adr/container/0009-replay-ring-c-abi.md`): no
//! encoder, no files. Synthetic packets whose payload names the frame they belong to.

#![cfg(test)]
#![allow(unsafe_code, clippy::unwrap_used, clippy::panic, reason = "unit tests")]

use super::*;

const VIDEO: u32 = 1;
const AUDIO: u32 = 2;
const FPS: MediawayRational = MediawayRational { num: 1, den: 30 };
const AUDIO_RATE: MediawayRational = MediawayRational {
    num: 1,
    den: 48_000,
};

fn config(kind: MediawayReplayPayloadKind, window_ms: u64) -> MediawayReplayRingConfig {
    MediawayReplayRingConfig {
        anchor_stream_id: VIDEO,
        anchor_time_base: FPS,
        window_ms,
        max_bytes: 0,
        payload_kind: kind,
    }
}

/// The byte a frame's payload is made of: the frame number, modulo 256.
fn frame_byte(n: i64) -> u8 {
    u8::try_from(n.rem_euclid(256)).unwrap()
}

fn create(config: &MediawayReplayRingConfig) -> *mut ReplayRingHandle {
    let mut ring = std::ptr::null_mut();
    let status = unsafe { mediaway_replay_ring_create(config, &raw mut ring) };
    assert_eq!(status, MediawayStatus::Ok);
    assert!(!ring.is_null());
    ring
}

/// Frame `n` of a 30 fps stream with a keyframe every 30 frames. The payload is `[n as u8; 4]`
/// (mod 256), so a clip's bytes say which frame they are.
fn video_view(n: i64, payload: &[u8]) -> MediawayPacketView {
    MediawayPacketView {
        stream_id: VIDEO,
        pts: n,
        dts: n,
        duration: 1,
        is_keyframe: n % 30 == 0,
        is_discard: false,
        payload: payload.as_ptr(),
        payload_len: payload.len(),
    }
}

fn push_video(ring: *mut ReplayRingHandle, n: i64) -> MediawayStatus {
    let payload = [frame_byte(n); 4];
    let view = video_view(n, &payload);
    unsafe { mediaway_replay_ring_push(ring, &raw const view) }
}

fn push_audio(ring: *mut ReplayRingHandle, k: i64) -> MediawayStatus {
    let payload = [0xA0u8; 3];
    let view = MediawayPacketView {
        stream_id: AUDIO,
        pts: k * 1024,
        dts: k * 1024,
        duration: 1024,
        is_keyframe: true,
        is_discard: false,
        payload: payload.as_ptr(),
        payload_len: payload.len(),
    };
    unsafe { mediaway_replay_ring_push(ring, &raw const view) }
}

fn entries(clip: *const ReplayClipHandle) -> Vec<MediawayReplayClipEntry> {
    let n = unsafe { mediaway_replay_clip_packet_count(clip) };
    (0..n)
        .map(|i| {
            let mut e = unsafe { std::mem::zeroed::<MediawayReplayClipEntry>() };
            let status = unsafe { mediaway_replay_clip_packet_at(clip, i, &raw mut e) };
            assert_eq!(status, MediawayStatus::Ok);
            e
        })
        .collect()
}

fn clip_last(ring: *mut ReplayRingHandle, span_ms: u64) -> Option<*mut ReplayClipHandle> {
    let mut clip = std::ptr::null_mut();
    let mut has = true;
    let status =
        unsafe { mediaway_replay_ring_clip_last(ring, span_ms, &raw mut clip, &raw mut has) };
    assert_eq!(status, MediawayStatus::Ok);
    assert_eq!(has, !clip.is_null());
    has.then_some(clip)
}

#[test]
fn a_clip_starts_at_a_keyframe_rebased_to_zero_and_carries_every_stream() {
    let ring = create(&config(MediawayReplayPayloadKind::Bytes, 2_000));
    let status = unsafe { mediaway_replay_ring_add_stream(ring, AUDIO, AUDIO_RATE) };
    assert_eq!(status, MediawayStatus::Ok);

    // 6 s of video, with the matching audio pushed alongside it.
    for n in 0..180 {
        assert_eq!(push_video(ring, n), MediawayStatus::Ok);
        if n % 2 == 0 {
            assert_eq!(push_audio(ring, n / 2 * 3), MediawayStatus::Ok);
        }
    }
    let mut span = 0u64;
    assert_eq!(
        unsafe { mediaway_replay_ring_span_ms(ring, &raw mut span) },
        MediawayStatus::Ok
    );
    assert!(
        span >= 2_000,
        "the ring must hold at least its window, holds {span} ms"
    );

    let Some(clip) = clip_last(ring, 2_000) else {
        panic!("no clip although keyframes were pushed");
    };
    let list = entries(clip);
    assert!(!list.is_empty());

    // The anchor's first packet is a keyframe that decodes at zero.
    let first_video = list.iter().find(|e| e.stream_id == VIDEO).unwrap();
    assert!(first_video.is_keyframe);
    assert_eq!((first_video.pts, first_video.dts), (0, 0));
    assert!(
        list.iter().any(|e| e.stream_id == AUDIO),
        "audio was cut with the video"
    );

    // Decode order, and the payload is the frame it claims to be: frame = dts + the cut.
    let video: Vec<_> = list.iter().filter(|e| e.stream_id == VIDEO).collect();
    for pair in video.windows(2) {
        assert_eq!(pair[1].dts, pair[0].dts + 1);
    }
    let cut_frame = 180 - i64::try_from(video.len()).unwrap();
    assert_eq!(
        cut_frame % 30,
        0,
        "the clip starts on a GOP boundary, frame {cut_frame}"
    );
    for e in &video {
        assert_eq!(e.payload_kind, MediawayReplayPayloadKind::Bytes);
        let bytes = unsafe { std::slice::from_raw_parts(e.payload, e.payload_len) };
        assert_eq!(bytes, [frame_byte(cut_frame + e.dts); 4]);
    }

    let mut ms = 0u64;
    assert_eq!(
        unsafe { mediaway_replay_clip_duration_ms(clip, &raw mut ms) },
        MediawayStatus::Ok
    );
    assert!(ms >= 2_000, "a clip is at least as long as asked, {ms} ms");
    unsafe { mediaway_replay_clip_free(clip) };
    unsafe { mediaway_replay_ring_close(ring) };
}

/// The Rust `Clip` borrows the ring. The C one must not: pushing (which evicts) and even closing
/// the ring must leave a clip that was already taken intact.
#[test]
fn a_clip_is_an_owned_snapshot_that_outlives_pushes_and_the_ring() {
    let ring = create(&config(MediawayReplayPayloadKind::Bytes, 1_000));
    for n in 0..90 {
        assert_eq!(push_video(ring, n), MediawayStatus::Ok);
    }
    let clip = clip_last(ring, 1_000).unwrap();
    let before: Vec<(i64, Vec<u8>)> = entries(clip)
        .iter()
        .map(|e| {
            (
                e.dts,
                unsafe { std::slice::from_raw_parts(e.payload, e.payload_len) }.to_vec(),
            )
        })
        .collect();

    // Evict everything the clip was cut from, then drop the ring.
    for n in 90..400 {
        assert_eq!(push_video(ring, n), MediawayStatus::Ok);
    }
    unsafe { mediaway_replay_ring_close(ring) };

    let after: Vec<(i64, Vec<u8>)> = entries(clip)
        .iter()
        .map(|e| {
            (
                e.dts,
                unsafe { std::slice::from_raw_parts(e.payload, e.payload_len) }.to_vec(),
            )
        })
        .collect();
    assert_eq!(before, after);
    unsafe { mediaway_replay_clip_free(clip) };
}

#[test]
fn no_clip_until_the_anchor_has_a_keyframe() {
    let ring = create(&config(MediawayReplayPayloadKind::Bytes, 1_000));
    // Frame 5 is not a keyframe, and nothing can be decoded from before one: dropped, no error.
    assert_eq!(push_video(ring, 5), MediawayStatus::Ok);
    assert!(clip_last(ring, 1_000).is_none());
    let mut span = 9u64;
    assert_eq!(
        unsafe { mediaway_replay_ring_span_ms(ring, &raw mut span) },
        MediawayStatus::Ok
    );
    assert_eq!(span, 0);
    unsafe { mediaway_replay_ring_close(ring) };
}

#[test]
fn a_stored_ring_hands_back_locations_not_bytes() {
    let ring = create(&config(MediawayReplayPayloadKind::Stored, 2_000));
    for n in 0..100i64 {
        let meta = MediawayPacketMeta {
            stream_id: VIDEO,
            pts: n,
            dts: n,
            duration: 1,
            is_keyframe: n % 30 == 0,
            is_discard: false,
        };
        let stored = MediawayStoredPayload {
            file: 7,
            offset: u64::try_from(n).unwrap() * 100,
            len: 40,
        };
        let status =
            unsafe { mediaway_replay_ring_push_stored(ring, &raw const meta, &raw const stored) };
        assert_eq!(status, MediawayStatus::Ok);
    }
    let clip = clip_last(ring, 1_000).unwrap();
    let list = entries(clip);
    assert!(!list.is_empty());
    let cut = 99 - list.last().unwrap().dts;
    for e in &list {
        assert_eq!(e.payload_kind, MediawayReplayPayloadKind::Stored);
        assert!(e.payload.is_null() && e.payload_len == 0);
        assert_eq!(e.stored_file, 7);
        assert_eq!(e.stored_len, 40);
        assert_eq!(e.stored_offset, u64::try_from(cut + e.dts).unwrap() * 100);
    }
    unsafe { mediaway_replay_clip_free(clip) };
    unsafe { mediaway_replay_ring_close(ring) };
}

#[test]
fn pushing_the_wrong_payload_kind_is_invalid_state() {
    let bytes_ring = create(&config(MediawayReplayPayloadKind::Bytes, 1_000));
    let stored_ring = create(&config(MediawayReplayPayloadKind::Stored, 1_000));
    let meta = MediawayPacketMeta {
        stream_id: VIDEO,
        pts: 0,
        dts: 0,
        duration: 1,
        is_keyframe: true,
        is_discard: false,
    };
    let stored = MediawayStoredPayload {
        file: 0,
        offset: 0,
        len: 1,
    };
    assert_eq!(
        unsafe { mediaway_replay_ring_push_stored(bytes_ring, &raw const meta, &raw const stored) },
        MediawayStatus::InvalidState
    );
    assert_eq!(push_video(stored_ring, 0), MediawayStatus::InvalidState);
    unsafe {
        mediaway_replay_ring_close(bytes_ring);
        mediaway_replay_ring_close(stored_ring);
    }
}

#[test]
fn a_ring_refuses_what_it_cannot_hold_and_says_why() {
    let ring = create(&config(MediawayReplayPayloadKind::Bytes, 1_000));
    // dts went backwards: refused, and the caller can carry on. (Anchor packets before the first
    // keyframe are dropped before any order check, so start on frame 0.)
    assert_eq!(push_video(ring, 0), MediawayStatus::Ok);
    assert_eq!(push_video(ring, 10), MediawayStatus::Ok);
    assert_eq!(push_video(ring, 4), MediawayStatus::InvalidPacket);
    assert_eq!(push_video(ring, 11), MediawayStatus::Ok);
    // A stream nobody added.
    assert_eq!(push_audio(ring, 0), MediawayStatus::UnknownStream);
    // The anchor is already there.
    assert_eq!(
        unsafe { mediaway_replay_ring_add_stream(ring, VIDEO, FPS) },
        MediawayStatus::InvalidTrack
    );
    // A degenerate timebase, on a new stream and on create.
    let zero = MediawayRational { num: 0, den: 1 };
    assert_eq!(
        unsafe { mediaway_replay_ring_add_stream(ring, AUDIO, zero) },
        MediawayStatus::InvalidArgument
    );
    let mut bad = config(MediawayReplayPayloadKind::Bytes, 1_000);
    bad.anchor_time_base = zero;
    let mut out = std::ptr::null_mut();
    assert_eq!(
        unsafe { mediaway_replay_ring_create(&raw const bad, &raw mut out) },
        MediawayStatus::InvalidArgument
    );
    assert!(out.is_null());
    unsafe { mediaway_replay_ring_close(ring) };
}

#[test]
fn a_byte_ceiling_evicts_oldest_gop_first() {
    let mut capped = config(MediawayReplayPayloadKind::Bytes, 60_000);
    capped.max_bytes = 30 * 4 * 2; // about two GOPs of 4-byte frames
    let ring = create(&capped);
    for n in 0..300 {
        assert_eq!(push_video(ring, n), MediawayStatus::Ok);
    }
    let clip = clip_last(ring, 60_000).unwrap();
    let held = unsafe { mediaway_replay_clip_packet_count(clip) };
    assert!(
        held <= 3 * 30,
        "a ceiling of two GOPs must bound the ring well below the 300 pushed, holds {held}"
    );
    unsafe { mediaway_replay_clip_free(clip) };
    unsafe { mediaway_replay_ring_close(ring) };
}

#[test]
fn null_and_out_of_range_arguments_are_refused_not_dereferenced() {
    let mut ring = std::ptr::null_mut();
    assert_eq!(
        unsafe { mediaway_replay_ring_create(std::ptr::null(), &raw mut ring) },
        MediawayStatus::InvalidArgument
    );
    let view = video_view(0, &[1]);
    assert_eq!(
        unsafe { mediaway_replay_ring_push(std::ptr::null_mut(), &raw const view) },
        MediawayStatus::InvalidArgument
    );
    let ring = create(&config(MediawayReplayPayloadKind::Bytes, 1_000));
    assert_eq!(push_video(ring, 0), MediawayStatus::Ok);
    let clip = clip_last(ring, 1_000).unwrap();
    let mut e = unsafe { std::mem::zeroed::<MediawayReplayClipEntry>() };
    assert_eq!(
        unsafe { mediaway_replay_clip_packet_at(clip, 99, &raw mut e) },
        MediawayStatus::InvalidArgument
    );
    assert_eq!(
        unsafe { mediaway_replay_clip_packet_count(std::ptr::null()) },
        0
    );
    // Freeing nothing is always safe.
    unsafe {
        mediaway_replay_clip_free(std::ptr::null_mut());
        mediaway_replay_ring_close(std::ptr::null_mut());
        mediaway_replay_clip_free(clip);
        mediaway_replay_ring_close(ring);
    }
}

#[test]
fn the_c_structs_have_the_layout_the_header_documents() {
    use std::mem::{offset_of, size_of};
    // Pinned so a reordered field fails here instead of corrupting a binding's mirror.
    assert_eq!(size_of::<MediawayReplayPayloadKind>(), 4);
    assert_eq!(offset_of!(MediawayReplayRingConfig, anchor_stream_id), 0);
    assert_eq!(size_of::<MediawayStoredPayload>(), 24); // u32, pad, u64, u32, pad
    assert_eq!(offset_of!(MediawayStoredPayload, offset), 8);
    assert_eq!(size_of::<MediawayPacketMeta>(), 40);
    assert_eq!(offset_of!(MediawayReplayClipEntry, stream_id), 0);
    assert_eq!(offset_of!(MediawayReplayClipEntry, pts), 8);
}
