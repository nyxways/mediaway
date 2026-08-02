//! Integration: public API mux → demux round trip.

#![forbid(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration tests may unwrap"
)]

use mpeg_audio::{ChannelMode, Demuxer, FrameHeader, MpegVersion, Muxer};

#[test]
fn mpeg2_layer3_roundtrip_via_public_api() {
    let header = FrameHeader {
        version: MpegVersion::Mpeg2,
        bitrate_kbps: 64,
        sample_rate: 22_050,
        channel_mode: ChannelMode::JointStereo,
    };
    let mux = Muxer::new(header).expect("mux");
    let body = vec![0x9Au8; header.frame_len(false) - 4];

    let mut bytes = Vec::new();
    mux.write_frame(&body, false, &mut bytes)
        .expect("write_frame");

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    let frame = demux.poll_frame().expect("poll_frame").expect("frame");
    assert_eq!(&frame[..], &body[..]);
    assert_eq!(demux.header(), Some(header));
}
