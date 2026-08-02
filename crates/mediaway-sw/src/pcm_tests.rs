#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;
use mediaway_common::Bytes as CommonBytes;

fn format() -> PcmFormat {
    PcmFormat {
        sample_rate: 48_000,
        channels: 2,
        sample_format: SampleFormat::S16,
    }
}

fn config() -> PcmPassthroughConfig {
    PcmPassthroughConfig::new(format(), Rational::new(1, 48_000))
}

fn frame(pts: i64, payload: &[u8]) -> AudioFrame {
    AudioFrame {
        pts,
        duration: payload.len() as u64 / 4,
        sample_rate: format().sample_rate,
        channels: format().channels,
        format: format().sample_format,
        data: CommonBytes::copy_from_slice(payload),
    }
}

#[test]
fn encoder_passes_payload_through_unchanged() {
    let mut enc = PcmEncoder::new(config());
    let payload = [1u8, 2, 3, 4, 5, 6, 7, 8];
    enc.push_frame(&frame(10, &payload)).unwrap();

    let packet = enc.poll_packet().unwrap().expect("packet ready");
    assert_eq!(packet.payload.as_ref(), &payload[..]);
    assert_eq!(packet.pts, 10);
    assert_eq!(packet.dts, 10);
    assert_eq!(packet.duration, 2);
    assert!(packet.is_keyframe);
    assert!(!packet.is_discard);
    assert!(enc.poll_packet().unwrap().is_none());
}

#[test]
fn encoder_queues_multiple_frames_in_fifo_order() {
    let mut enc = PcmEncoder::new(config());
    enc.push_frame(&frame(0, &[1, 2, 3, 4])).unwrap();
    enc.push_frame(&frame(1, &[5, 6, 7, 8])).unwrap();

    let first = enc.poll_packet().unwrap().expect("first packet");
    let second = enc.poll_packet().unwrap().expect("second packet");
    assert_eq!(first.pts, 0);
    assert_eq!(second.pts, 1);
    assert!(enc.poll_packet().unwrap().is_none());
}

#[test]
fn encoder_rejects_sample_format_mismatch() {
    let mut enc = PcmEncoder::new(config());
    let mut f = frame(0, &[1, 2, 3, 4]);
    f.format = SampleFormat::F32;
    assert_eq!(enc.push_frame(&f), Err(PcmError::SampleFormatMismatch));
}

#[test]
fn encoder_rejects_sample_rate_mismatch() {
    let mut enc = PcmEncoder::new(config());
    let mut f = frame(0, &[1, 2, 3, 4]);
    f.sample_rate = 44_100;
    assert_eq!(enc.push_frame(&f), Err(PcmError::SampleRateMismatch));
}

#[test]
fn encoder_rejects_channel_count_mismatch() {
    let mut enc = PcmEncoder::new(config());
    let mut f = frame(0, &[1, 2, 3, 4]);
    f.channels = 1;
    assert_eq!(enc.push_frame(&f), Err(PcmError::ChannelCountMismatch));
}

#[test]
fn encoder_rejects_push_after_flush() {
    let mut enc = PcmEncoder::new(config());
    enc.flush().unwrap();
    assert_eq!(
        enc.push_frame(&frame(0, &[1, 2, 3, 4])),
        Err(PcmError::Closed)
    );
}

#[test]
fn decoder_passes_payload_through_unchanged() {
    let mut dec = PcmDecoder::new(config());
    let payload = [9u8, 8, 7, 6];
    let packet = Packet {
        stream_id: 0,
        pts: 42,
        dts: 42,
        duration: 1,
        is_keyframe: true,
        is_discard: false,
        payload: CommonBytes::copy_from_slice(&payload),
    };
    dec.push_packet(&packet).unwrap();

    let out = dec.poll_frame().unwrap().expect("frame ready");
    assert_eq!(out.data.as_ref(), &payload[..]);
    assert_eq!(out.pts, 42);
    assert_eq!(out.sample_rate, format().sample_rate);
    assert_eq!(out.channels, format().channels);
    assert_eq!(out.format, format().sample_format);
    assert!(dec.poll_frame().unwrap().is_none());
}

#[test]
fn decoder_rejects_push_after_flush() {
    let mut dec = PcmDecoder::new(config());
    dec.flush().unwrap();
    let packet = Packet {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 1,
        is_keyframe: true,
        is_discard: false,
        payload: CommonBytes::copy_from_slice(&[1, 2, 3, 4]),
    };
    assert_eq!(dec.push_packet(&packet), Err(PcmError::Closed));
}

#[test]
fn stream_info_reports_configured_format() {
    let enc = PcmEncoder::new(config());
    let info = enc.stream_info();
    assert_eq!(info.codec(), CodecKind::RawAudio);
    assert_eq!(info.sample_rate(), Some(format().sample_rate));
    assert_eq!(info.channels(), Some(format().channels));
}
