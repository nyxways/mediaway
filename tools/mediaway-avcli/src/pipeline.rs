//! Mux pipeline: packets in (synthetic self-test or one access unit from a
//! byte source) → MP4 bytes out. No real H.264 encoder is wired yet — see
//! `adr/0001-avcli-flag-subset.md`.

use crate::args::Geometry;
use crate::error::CliError;
use bytes::Bytes;
use mediaway_common::{CodecKind, Packet, Rational, StreamInfo, VideoGeometry};
use mediaway_container::mp4::Muxer;

/// Canned SPS+PPS+IDR-slice Annex-B bytes (not a real encoder — see module docs).
const SPS_PPS_IDR_NAL: &[u8] = &[
    0x00, 0x00, 0x00, 0x01, 0x67, 0x42, 0x00, 0x1e, 0xab, 0x40, 0xf0, 0x28, 0xd3, 0x70, 0x00, 0x00,
    0x00, 0x01, 0x68, 0xce, 0x3c, 0x80,
];
/// Canned non-IDR slice Annex-B bytes.
const SLICE_NAL: &[u8] = &[
    0x00, 0x00, 0x00, 0x01, 0x65, 0xb8, 0x00, 0x04, 0x00, 0x00, 0x13, 0x88, 0x00, 0x00, 0x03, 0x00,
];

/// 1ms-tick timebase shared by both mux modes (30fps-shaped synthetic timing).
const TIME_BASE: Rational = Rational::new(1, 1000);

const fn video_track(geometry: Geometry) -> StreamInfo {
    StreamInfo::Video {
        id: 0,
        codec: CodecKind::H264,
        time_base: TIME_BASE,
        geometry: VideoGeometry {
            width: geometry.width,
            height: geometry.height,
        },
        extra_data: Bytes::new(),
    }
}

/// Mux `count` synthetic H.264 packets (Mediaway self-test mode; not real
/// encoded video) at a 33ms/packet cadence into MP4 bytes.
///
/// # Errors
///
/// Track registration or per-packet mux failure.
pub(crate) fn mux_synthetic(count: usize, geometry: Geometry) -> Result<Vec<u8>, CliError> {
    let mut open = Muxer::new();
    open.add_track(video_track(geometry))?;
    let mut muxer = open.begin();

    let mut out = Vec::with_capacity(1024 * 1024);
    for i in 0..count {
        let mut payload = Vec::new();
        if i == 0 {
            payload.extend_from_slice(SPS_PPS_IDR_NAL);
        }
        payload.extend_from_slice(SLICE_NAL);

        let ts = i64::try_from(i).unwrap_or(i64::MAX).saturating_mul(33);
        let pkt = Packet {
            stream_id: 0,
            pts: ts,
            dts: ts,
            duration: 33,
            is_keyframe: i == 0 || i % 30 == 0,
            is_discard: false,
            payload: Bytes::from(payload),
        };
        muxer.push_packet(&pkt)?;
        muxer.poll_bytes(&mut out);
    }
    muxer.flush();
    muxer.poll_bytes(&mut out);
    Ok(out)
}

/// Mux `access_unit` as a single keyframe packet into MP4 bytes.
///
/// Mediaway has no elementary-stream access-unit splitter exposed yet, so the
/// whole input is treated as one packet — this mirrors the crate's prior
/// `--stdin` scaffold behavior, generalized to any byte source.
///
/// # Errors
///
/// Track registration or mux failure.
pub(crate) fn mux_single_access_unit(
    access_unit: Vec<u8>,
    geometry: Geometry,
) -> Result<Vec<u8>, CliError> {
    let mut open = Muxer::with_fragment_batch(1);
    open.add_track(video_track(geometry))?;
    let mut muxer = open.begin();

    let pkt = Packet {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 33,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::from(access_unit),
    };
    muxer.push_packet(&pkt)?;
    muxer.flush();
    let mut out = Vec::new();
    muxer.poll_bytes(&mut out);
    Ok(out)
}
