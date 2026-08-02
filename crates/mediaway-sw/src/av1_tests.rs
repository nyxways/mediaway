#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "synthetic test-frame byte generation and OBU-length test parsing only; \
              values are small/bounded fixture sizes, not real codec input"
)]

use super::*;
use mediaway_common::{Bytes as CommonBytes, GpuBufferHandle};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;

fn config() -> Av1EncoderConfig {
    let mut cfg = Av1EncoderConfig::new(WIDTH, HEIGHT, Rational::new(1, 30));
    // Fastest preset: keeps the default test suite quick. Correctness of the
    // adapter (framing, config plumbing, error mapping) does not depend on
    // encode quality.
    cfg.speed = 10;
    cfg
}

/// Synthetic I420 frame: a diagonal gradient so the encoder sees real
/// structure instead of a degenerate flat block (generated in-test per
/// `docs/conventions/testing.md` — no committed media binaries).
fn gradient_frame(pts: i64) -> VideoFrame {
    let w = WIDTH as usize;
    let h = HEIGHT as usize;
    let (cw, ch) = ChromaSampling::Cs420.get_chroma_dimensions(w, h);

    let mut data = Vec::with_capacity(w * h + 2 * cw * ch);
    for y in 0..h {
        for x in 0..w {
            data.push(((x + y + pts as usize * 7) % 256) as u8);
        }
    }
    data.resize(data.len() + cw * ch, 96); // U
    data.resize(data.len() + cw * ch, 160); // V

    VideoFrame {
        pts,
        duration: 1,
        width: WIDTH,
        height: HEIGHT,
        format: PixelFormat::I420,
        storage: VideoFrameStorage::Cpu {
            data: CommonBytes::from(data),
        },
    }
}

/// Walks AV1 "low overhead bitstream format" OBUs in `data` and returns the
/// `obu_type` (AV1 spec § 5.3.2) of each one found. Stops (rather than
/// panicking) on any OBU without an explicit size field or on a malformed
/// length, since this is a structural test helper, not a conformance parser.
fn obu_types(mut data: &[u8]) -> Vec<u8> {
    let mut types = Vec::new();
    while !data.is_empty() {
        let header = data[0];
        let obu_type = (header >> 3) & 0x0F;
        let ext_flag = (header >> 2) & 1 == 1;
        let has_size = (header >> 1) & 1 == 1;
        types.push(obu_type);

        let mut pos = 1 + usize::from(ext_flag);
        if !has_size || pos > data.len() {
            break;
        }
        let (obu_size, leb_len) = read_leb128(&data[pos..]);
        pos += leb_len;
        let end = pos + obu_size as usize;
        if leb_len == 0 || end > data.len() {
            break;
        }
        data = &data[end..];
    }
    types
}

fn read_leb128(data: &[u8]) -> (u64, usize) {
    let mut value: u64 = 0;
    let mut i = 0;
    while i < data.len() && i < 8 {
        let byte = data[i];
        value |= u64::from(byte & 0x7f) << (i * 7);
        i += 1;
        if byte & 0x80 == 0 {
            return (value, i);
        }
    }
    (0, 0)
}

const OBU_SEQUENCE_HEADER: u8 = 1;

#[test]
fn open_accepts_a_valid_config() {
    assert!(Av1Encoder::open(&config()).is_ok());
}

#[test]
fn open_reports_sequence_header_in_stream_info_immediately() {
    let enc = Av1Encoder::open(&config()).unwrap();
    let StreamInfo::Video { extra_data, .. } = enc.stream_info() else {
        unreachable!("Av1Encoder always reports a video stream");
    };
    assert!(!extra_data.is_empty());
}

#[test]
fn push_frame_rejects_non_i420_pixel_format() {
    let mut enc = Av1Encoder::open(&config()).unwrap();
    let mut frame = gradient_frame(0);
    frame.format = PixelFormat::Nv12;
    assert_eq!(enc.push_frame(&frame), Err(Av1Error::Unsupported));
}

#[test]
fn push_frame_rejects_gpu_storage() {
    let mut enc = Av1Encoder::open(&config()).unwrap();
    let mut frame = gradient_frame(0);
    frame.storage = VideoFrameStorage::Gpu(GpuBufferHandle::WebGpu { texture_id: 1 });
    assert_eq!(enc.push_frame(&frame), Err(Av1Error::Unsupported));
}

#[test]
fn push_frame_rejects_dimension_mismatch() {
    let mut enc = Av1Encoder::open(&config()).unwrap();
    let mut frame = gradient_frame(0);
    frame.width = WIDTH + 16;
    assert_eq!(enc.push_frame(&frame), Err(Av1Error::InvalidInput));
}

#[test]
fn push_frame_rejects_undersized_buffer() {
    let mut enc = Av1Encoder::open(&config()).unwrap();
    let frame = VideoFrame {
        pts: 0,
        duration: 1,
        width: WIDTH,
        height: HEIGHT,
        format: PixelFormat::I420,
        storage: VideoFrameStorage::Cpu {
            data: CommonBytes::from_static(b"too short"),
        },
    };
    assert_eq!(enc.push_frame(&frame), Err(Av1Error::InvalidInput));
}

#[test]
fn encode_a_few_frames_and_flush_produces_valid_looking_av1() {
    let mut enc = Av1Encoder::open(&config()).unwrap();

    for pts in 0..6i64 {
        enc.push_frame(&gradient_frame(pts)).unwrap();
    }
    enc.flush().unwrap();

    let mut packets = Vec::new();
    while let Some(packet) = enc.poll_packet().unwrap() {
        packets.push(packet);
    }

    assert!(!packets.is_empty(), "expected at least one AV1 packet");
    let total_bytes: usize = packets.iter().map(|p| p.payload.len()).sum();
    assert!(
        total_bytes > 64,
        "expected non-trivial encoded output, got {total_bytes} bytes"
    );
    assert!(packets.iter().any(|p| p.is_keyframe));

    let first_types = obu_types(&packets[0].payload);
    assert!(
        first_types.contains(&OBU_SEQUENCE_HEADER),
        "expected an OBU_SEQUENCE_HEADER in the first packet, got OBU types {first_types:?}"
    );
}

#[test]
fn push_frame_after_flush_is_rejected() {
    let mut enc = Av1Encoder::open(&config()).unwrap();
    enc.push_frame(&gradient_frame(0)).unwrap();
    enc.flush().unwrap();
    assert_eq!(enc.push_frame(&gradient_frame(1)), Err(Av1Error::Closed));
}

#[test]
fn flush_without_any_pushed_frames_does_not_hang() {
    let mut enc = Av1Encoder::open(&config()).unwrap();
    enc.flush().unwrap();
    // Must terminate — poll_packet's internal Encoded-retry loop must not
    // spin forever once rav1e has nothing left to encode.
    let mut drained = 0;
    while enc.poll_packet().unwrap().is_some() {
        drained += 1;
        assert!(drained < 1000, "poll_packet looped without draining");
    }
}
