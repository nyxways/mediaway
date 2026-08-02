//! Integration: public API mux → demux round trip over multiple frames.

#![forbid(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration tests may unwrap"
)]

use adts::{AacProfile, AdtsConfig, Demuxer, Muxer};

#[test]
fn multi_frame_roundtrip_via_public_api() {
    let config = AdtsConfig {
        profile: AacProfile::Lc,
        sample_rate: 44_100,
        channels: 2,
    };
    let mux = Muxer::new(config).expect("mux");
    let frames: [&[u8]; 3] = [&[1, 2, 3], &[4, 5], &[6, 7, 8, 9]];

    let mut bytes = Vec::new();
    for frame in frames {
        mux.write_frame(frame, &mut bytes).expect("write_frame");
    }

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    for frame in frames {
        let got = demux.poll_frame().expect("poll_frame").expect("frame");
        assert_eq!(&got[..], frame);
    }
    assert!(demux.poll_frame().expect("poll_frame").is_none());
    assert_eq!(demux.config(), Some(config));
}
