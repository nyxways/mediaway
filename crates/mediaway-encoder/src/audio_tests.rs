#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use mediaway_common::{AudioFrame, Bytes, CodecKind, Packet, Rational, SampleFormat, StreamInfo};

use super::AudioEncoder;
use crate::EncodeError;

/// Emits nothing until flushed, like an encoder waiting for a full frame of samples.
struct Buffering {
    info: StreamInfo,
    held: Vec<i64>,
    ready: Vec<i64>,
}

impl AudioEncoder for Buffering {
    fn stream_info(&self) -> &StreamInfo {
        &self.info
    }
    fn push_frame(&mut self, frame: &AudioFrame) -> Result<(), EncodeError> {
        self.held.push(frame.pts);
        Ok(())
    }
    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        Ok((!self.ready.is_empty()).then(|| {
            let pts = self.ready.remove(0);
            Packet {
                stream_id: 0,
                pts,
                dts: pts,
                duration: 1024,
                is_keyframe: true,
                is_discard: false,
                payload: Bytes::from(vec![0u8; 1]),
            }
        }))
    }
    fn flush(&mut self) -> Result<(), EncodeError> {
        self.ready.append(&mut self.held);
        Ok(())
    }
}

#[test]
fn finish_returns_the_buffered_audio() {
    let mut encoder = Buffering {
        info: StreamInfo::Audio {
            id: 0,
            codec: CodecKind::Aac,
            time_base: Rational::new(1, 48_000),
            sample_rate: 48_000,
            channels: 2,
            extra_data: Bytes::new(),
        },
        held: Vec::new(),
        ready: Vec::new(),
    };
    for pts in [0, 1024] {
        let frame = AudioFrame {
            pts,
            duration: 1024,
            sample_rate: 48_000,
            channels: 2,
            format: SampleFormat::S16,
            data: Bytes::from(vec![0u8; 4096]),
        };
        encoder.push_frame(&frame).unwrap();
    }
    assert!(
        encoder.poll_packet().unwrap().is_none(),
        "nothing before the flush"
    );
    let pts: Vec<i64> = encoder.finish().unwrap().iter().map(|p| p.pts).collect();
    assert_eq!(pts, [0, 1024]);
}
