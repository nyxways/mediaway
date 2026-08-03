//! Integration: public API mux → demux round trip.

#![forbid(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration tests may unwrap"
)]

use bytes::Bytes;
use flv_core::{Demuxer, Muxer, Tag, TagType};

#[test]
fn multi_tag_roundtrip_via_public_api() {
    let mut mux = Muxer::new();
    let mut bytes = Vec::new();
    mux.write_header(true, true, &mut bytes);

    let tags = [
        Tag {
            tag_type: TagType::Video,
            timestamp_ms: 0,
            data: Bytes::from_static(&[0x17, 0x00]),
        },
        Tag {
            tag_type: TagType::Audio,
            timestamp_ms: 23,
            data: Bytes::from_static(&[0xAF, 0x01]),
        },
    ];
    for tag in &tags {
        mux.write_tag(tag, &mut bytes).expect("write_tag");
    }

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    for tag in &tags {
        let got = demux.poll_tag().expect("poll_tag").expect("tag");
        assert_eq!(&got, tag);
    }
    assert_eq!(demux.has_audio(), Some(true));
    assert_eq!(demux.has_video(), Some(true));
}
