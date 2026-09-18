#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use mediaway_common::{
    Bytes, CodecKind, Packet, PixelFormat, Rational, StreamInfo, VideoFrame, VideoFrameStorage,
    VideoGeometry,
};

use super::VideoEncoder;
use crate::EncodeError;

/// Holds `DELAY` frames back until flushed, like a pipelined hardware encoder.
struct Pipelined {
    info: StreamInfo,
    held: Vec<i64>,
    ready: Vec<i64>,
}

const DELAY: usize = 2;

impl Pipelined {
    fn new() -> Self {
        Self {
            info: StreamInfo::Video {
                id: 0,
                codec: CodecKind::H264,
                time_base: Rational::new(1, 60),
                geometry: VideoGeometry {
                    width: 2,
                    height: 2,
                },
                extra_data: Bytes::new(),
            },
            held: Vec::new(),
            ready: Vec::new(),
        }
    }
}

fn packet(pts: i64) -> Packet {
    Packet {
        stream_id: 0,
        pts,
        dts: pts,
        duration: 1,
        is_keyframe: pts == 0,
        is_discard: false,
        payload: Bytes::from(vec![0u8; 1]),
    }
}

impl VideoEncoder for Pipelined {
    fn stream_info(&self) -> &StreamInfo {
        &self.info
    }
    fn push_frame(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        self.held.push(frame.pts);
        if self.held.len() > DELAY {
            self.ready.push(self.held.remove(0));
        }
        Ok(())
    }
    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        Ok((!self.ready.is_empty()).then(|| packet(self.ready.remove(0))))
    }
    fn flush(&mut self) -> Result<(), EncodeError> {
        self.ready.append(&mut self.held);
        Ok(())
    }
}

fn frame(pts: i64) -> VideoFrame {
    VideoFrame {
        pts,
        duration: 1,
        width: 2,
        height: 2,
        format: PixelFormat::Nv12,
        storage: VideoFrameStorage::Cpu {
            data: Bytes::from(vec![0u8; 6]),
        },
    }
}

#[test]
fn finish_returns_the_frames_still_inside_the_pipeline() {
    let mut encoder = Pipelined::new();
    for pts in 0..5 {
        encoder.push_frame(&frame(pts)).unwrap();
    }
    let mut polled = Vec::new();
    while let Some(p) = encoder.poll_packet().unwrap() {
        polled.push(p.pts);
    }
    assert_eq!(
        polled,
        [0, 1, 2],
        "the last {DELAY} frames are still in the pipeline"
    );
    let rest: Vec<i64> = encoder.finish().unwrap().iter().map(|p| p.pts).collect();
    assert_eq!(
        rest,
        [3, 4],
        "finish hands back exactly the unpolled tail, in order"
    );
}

#[test]
fn dropping_without_finish_loses_the_tail() {
    // The failure `finish` exists for, stated so it cannot quietly become untrue: with a
    // pipelined encoder, polling alone never yields the last frames.
    let mut encoder = Pipelined::new();
    for pts in 0..5 {
        encoder.push_frame(&frame(pts)).unwrap();
    }
    let mut polled = 0;
    while encoder.poll_packet().unwrap().is_some() {
        polled += 1;
    }
    drop(encoder);
    assert_eq!(polled, 5 - DELAY);
}

#[test]
fn finish_works_through_a_boxed_encoder() {
    let mut encoder: Box<dyn VideoEncoder> = Box::new(Pipelined::new());
    for pts in 0..3 {
        encoder.push_frame(&frame(pts)).unwrap();
    }
    assert_eq!(encoder.finish().unwrap().len(), 3);
}
