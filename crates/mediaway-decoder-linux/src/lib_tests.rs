#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests may unwrap"
)]

use super::*;
use mediaway_common::{CodecKind, PixelFormat, Rational};
use mediaway_decoder::VideoOutputPreference;

/// Attempts to open a real VA-API display and H.264 CPU-output decode session.
///
/// **Expected to skip in this development session** — see
/// [ADR-0001](../adr/0001-vaapi-h264-cpu-out.md)'s "zero real-hardware verification" caveat:
/// this box has no working `/dev/dri/renderD*` VA-API device (WSL2 here has broken VA-API /
/// software-only Vulkan; no real Linux GPU is exposed). The test exists so that on a real
/// Linux + VA-API machine (Stage 3 platform target), running this same suite verifies the
/// open path end to end instead of only compiling.
#[test]
fn open_vaapi_h264_cpu_or_skip() {
    let cfg = VideoDecoderConfig {
        codec: CodecKind::H264,
        width: 64,
        height: 64,
        time_base: Rational::new(1, 30),
        pixel_format: PixelFormat::Nv12,
        output: VideoOutputPreference::CpuFramesOk,
        gpu_device: None,
        extra_data: Bytes::new(),
    };
    let mut dec = match LinuxVideoDecoder::open(&cfg) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("skip: no VA-API display available ({e:?})");
            return;
        }
    };
    dec.flush().expect("flush without packets");
    assert!(dec.poll_frame().expect("poll").is_none());
}
