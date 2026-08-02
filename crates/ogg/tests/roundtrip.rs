//! Integration: public API mux → demux round trip over a small logical stream.

#![forbid(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_possible_wrap,
    reason = "integration tests may unwrap; loop index is always tiny"
)]

use ogg::{Demuxer, Muxer};

#[test]
fn three_packet_stream_roundtrip_via_public_api() {
    let mut mux = Muxer::new(123);
    let mut bytes = Vec::new();
    let packets: [&[u8]; 3] = [b"first", b"second-packet", b"third"];

    for (i, packet) in packets.iter().enumerate() {
        let eos = i == packets.len() - 1;
        mux.push_packet(packet, i as i64 * 100, eos, &mut bytes)
            .expect("push_packet");
    }

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    for (i, packet) in packets.iter().enumerate() {
        let got = demux.poll_packet().expect("poll_packet").expect("packet");
        assert_eq!(&got.data[..], *packet);
        assert_eq!(got.serial, 123);
        assert_eq!(got.bos, i == 0);
        assert_eq!(got.eos, i == packets.len() - 1);
    }
    assert!(demux.poll_packet().expect("poll_packet").is_none());
}
