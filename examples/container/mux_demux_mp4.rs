//! Mux + demux roundtrip — pure Rust, all platforms.
//!
//! Demonstrates the typestate muxer (`Open` → `Live`) and the streaming demuxer.
//! No encoder, no OS codec, no `unsafe`. This is `mediaway-container`'s `mp4`
//! module in isolation — see `pipeline/encode_to_mp4.rs` for the version that
//! also encodes real video before muxing it.
//!
//! Run:
//! ```text
//! cargo run --example mux_demux_mp4
//! ```

#![allow(
    clippy::print_stdout,
    clippy::expect_used,
    reason = "example demonstrates the happy path with console output"
)]

use mediaway_common::{Bytes, CodecKind, Packet, Rational, StreamInfo, VideoGeometry};
use mediaway_container::mp4;

fn main() {
    let fps = Rational::new(1, 30);
    let sample_rate = Rational::new(1, 48_000);
    let frame_count: u32 = 90; // 3 s at 30 fps

    // ── 1. Register tracks (Open state) ──────────────────────────────────────
    let mut muxer = mp4::Muxer::new();

    let v_track = muxer
        .add_track(StreamInfo::Video {
            id: 0,
            codec: CodecKind::H264,
            time_base: fps,
            geometry: VideoGeometry {
                width: 1920,
                height: 1080,
            },
            extra_data: Bytes::new(),
        })
        .expect("add video track");

    let a_track = muxer
        .add_track(StreamInfo::Audio {
            id: 1,
            codec: CodecKind::Aac,
            time_base: sample_rate,
            extra_data: Bytes::new(),
            sample_rate: 48_000,
            channels: 2,
        })
        .expect("add audio track");

    // ── 2. Transition to streaming (Live state) ───────────────────────────────
    // After this point track registration is closed; packet submission begins.
    let mut muxer = muxer.begin();

    for i in 0..frame_count {
        muxer
            .push_packet(&Packet {
                stream_id: v_track,
                pts: i64::from(i),
                dts: i64::from(i),
                duration: 1,
                is_keyframe: i % 30 == 0,
                is_discard: false,
                payload: Bytes::from_static(b"\x00\x00\x00\x01"),
            })
            .expect("video packet");

        muxer
            .push_packet(&Packet {
                stream_id: a_track,
                pts: i64::from(i) * 1_600,
                dts: i64::from(i) * 1_600,
                duration: 1_600,
                is_keyframe: true,
                is_discard: false,
                payload: Bytes::from_static(b"\xff\xf1"),
            })
            .expect("audio packet");
    }
    muxer.flush();

    // ── 3. Pull bytes — caller owns I/O (no file handles inside the muxer) ───
    let mut mp4_bytes: Vec<u8> = Vec::new();
    muxer.poll_bytes(&mut mp4_bytes);
    println!(
        "mux_demux_mp4: {} frames → {} bytes of fMP4",
        frame_count,
        mp4_bytes.len()
    );

    // ── 4. Demux the same bytes back ─────────────────────────────────────────
    let mut demux = mp4::Demuxer::new();
    demux.push_bytes(&mp4_bytes);

    println!(
        "mux_demux_mp4: demuxer sees {} stream(s)",
        demux.streams().len()
    );
    for s in demux.streams() {
        match s.geometry() {
            Some(g) => println!(
                "  stream {} — {:?} {}×{}",
                s.id(),
                s.codec(),
                g.width,
                g.height
            ),
            None => println!("  stream {} — {:?} (no geometry)", s.id(), s.codec()),
        }
    }

    let mut n_video = 0u32;
    let mut n_audio = 0u32;
    while let Some(pkt) = demux.poll_packet() {
        if pkt.stream_id == v_track {
            n_video += 1;
        } else {
            n_audio += 1;
        }
    }
    println!("mux_demux_mp4: recovered {n_video} video + {n_audio} audio packets");
    assert!(n_video > 0);
    println!("mux_demux_mp4: OK");
}
