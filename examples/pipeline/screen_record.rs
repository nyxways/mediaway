//! Screen recording pipeline: capture → encode → fragmented MP4, video **and**
//! audio.
//!
//! `platform::ScreenCapture` / `platform::Microphone` / `platform::AutoEncoder`
//! handle OS dispatch internally — no `#[cfg(…)]` in this file for those.
//! Audio encode has no cross-platform dispatcher yet, so this reaches for
//! `mediaway_encoder::windows::WindowsAudioEncoder` directly; it compiles and
//! degrades gracefully (`EncodeError::Unsupported`) on every platform, same
//! as the video/capture backends' own `NoBackend`/`Unsupported` paths.
//!
//! Screen, mic, and both encoders are all **required** — if any one of them
//! is unavailable, the example skips with an honest message rather than
//! silently downgrading to video-only. That mirrors
//! `crates/mediaway-pipeline/tests/screen_mic_av_smoke.rs`, the tested
//! reference this example's mux shape is based on: `Option<&mut dyn Trait>`
//! plumbing for an optional participant makes the borrow checker (and the
//! reader) work much harder than a plain `&mut dyn Trait` does, for a
//! fallback mode nothing here actually exercises.
//!
//! [`mediaway_pipeline::EncodeSession`] stays video-only/single-track by
//! design ([ADR-0014](../../docs/adr/0014-pipeline-convenience-crate.md)) —
//! rather than extend it, this composes the second (audio) track directly
//! against a shared `mp4::Muxer`, the same multi-track pattern the smoke
//! test above uses (that test also covers the Zero-Copy DX11 capture path,
//! which needs a caller-owned `ID3D11Device` and therefore `unsafe` — out of
//! scope for a plain example).
//!
//! Video frames here are still a synthetic grey placeholder (captured BGRA →
//! NV12 conversion is a separate, unimplemented piece — see the note below);
//! audio is the real captured microphone signal, encoded to AAC.
//!
//! Run:
//! ```text
//! cargo run --example screen_record
//! ```
//! Output: `out_screen.mp4` in the current directory.

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::expect_used,
    reason = "example demonstrates the happy path with console output"
)]

use mediaway_common::{
    Bytes, CodecKind, PixelFormat, Rational, SampleFormat, VideoFrame, VideoFrameStorage,
    VideoGeometry,
};
use mediaway_container::mp4::Muxer;
use mediaway_device::Select;
use mediaway_device::audio::{AudioCapture, AudioCaptureConfig};
use mediaway_device::desktop::{DesktopVideoCapture, DesktopVideoCaptureConfig};
use mediaway_encoder::auto::AutoVideoEncodeConfig;
use mediaway_encoder::windows::WindowsAudioEncoder;
use mediaway_encoder::{AudioEncoder, AudioEncoderConfig, VideoEncoder};
use mediaway_pipeline::platform;
use std::fs::File;
use std::io::Write as _;
use std::time::{Duration, Instant};

const CAPTURE_SECS: u64 = 3;

fn main() {
    let fps = 30u32;
    let tb = Rational::new(1, fps);

    // ── Open every backend up front — skip honestly if any one is missing ────
    let mut cap = match platform::ScreenCapture::open(&DesktopVideoCaptureConfig::screen(
        Select::Default,
        tb,
    )) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("screen_record: capture unavailable ({e}) — platform not supported yet");
            return;
        }
    };

    let mut mic =
        match platform::Microphone::open(&AudioCaptureConfig::microphone(Rational::new(1, 48_000)))
        {
            Ok(m) => m,
            Err(e) => {
                eprintln!("screen_record: mic unavailable ({e})");
                return;
            }
        };

    let geometry = cap.stream_info().geometry().unwrap_or(VideoGeometry {
        width: 0,
        height: 0,
    });
    let (width, height) = (geometry.width, geometry.height);
    println!("screen_record: {width}×{height} display");

    let enc_cfg = AutoVideoEncodeConfig {
        bitrate_bps: 8_000_000,
        ..AutoVideoEncodeConfig::new(CodecKind::H264, width, height, tb)
    };
    let mut venc = match platform::AutoEncoder::open(&enc_cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("screen_record: video encoder unavailable ({e})");
            return;
        }
    };

    let Some(sample_rate) = mic.stream_info().sample_rate() else {
        eprintln!("screen_record: mic hasn't negotiated a sample rate yet");
        return;
    };
    let Some(channels) = mic.stream_info().channels() else {
        eprintln!("screen_record: mic hasn't negotiated a channel count yet");
        return;
    };
    let aenc_cfg = AudioEncoderConfig {
        codec: CodecKind::Aac,
        sample_rate,
        channels,
        sample_format: SampleFormat::F32,
        time_base: Rational::new(1, sample_rate),
        bitrate_bps: 128_000,
    };
    let mut aenc = match WindowsAudioEncoder::open(&aenc_cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("screen_record: audio encoder unavailable ({e:?})");
            return;
        }
    };

    // ── Core capture→encode loop ────────────────────────────────────────────
    let (vpackets, apackets) = record(
        cap.as_mut(),
        mic.as_mut(),
        &mut *venc,
        &mut aenc,
        width,
        height,
        Duration::from_secs(CAPTURE_SECS),
    );

    cap.close().ok();
    mic.close().ok();

    let mp4_bytes = mux_tracks(&*venc, &aenc, vpackets, apackets);
    File::create("out_screen.mp4")
        .and_then(|mut f| f.write_all(&mp4_bytes))
        .expect("write mp4");
    println!(
        "screen_record: → out_screen.mp4 ({} bytes)",
        mp4_bytes.len()
    );
}

/// Record for `duration` from `cap` + `mic`, feeding frames into `venc` /
/// `aenc`. `cap`/`venc` are **trait objects** so this function compiles
/// identically regardless of which OS backend `platform::ScreenCapture`/
/// `platform::AutoEncoder` opened above.
#[allow(
    clippy::too_many_arguments,
    reason = "example function, not public API"
)]
fn record(
    cap: &mut dyn DesktopVideoCapture,
    mic: &mut dyn AudioCapture,
    venc: &mut dyn VideoEncoder,
    aenc: &mut WindowsAudioEncoder,
    width: u32,
    height: u32,
    duration: Duration,
) -> (Vec<mediaway_common::Packet>, Vec<mediaway_common::Packet>) {
    let mut vpackets = Vec::new();
    let mut apackets = Vec::new();
    let deadline = Instant::now() + duration;
    let nv12_len = (width * height + width * height / 2) as usize;
    // Synthetic NV12 placeholder (Y=128, UV=128 → grey).
    // Not wired yet: real BGRA→NV12 conversion once the MFT path is in place.
    let grey_nv12 = Bytes::from(vec![128u8; nv12_len]);
    let mut video_pts = 0i64;

    while Instant::now() < deadline {
        // ── Video ─────────────────────────────────────────────────────────────
        match cap.poll_frame() {
            Ok(Some(_gpu_frame)) => {
                // _gpu_frame.storage = GpuBufferHandle::DirectX11 { texture, .. } (⚡)
                // Not wired yet: convert BGRA surface → NV12 MFT; feed to ZeroCopyGpu encoder.
                cap.release_frame().ok();

                let frame = VideoFrame {
                    pts: video_pts,
                    duration: 1,
                    width,
                    height,
                    format: PixelFormat::Nv12,
                    storage: VideoFrameStorage::Cpu {
                        // clone: Bytes ref-count bump (no memcpy of pixel buffer)
                        data: grey_nv12.clone(),
                    },
                };
                video_pts += 1;
                if venc.push_frame(&frame).is_ok() {
                    drain_video(venc, &mut vpackets);
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("screen_record: capture error ({e})");
                break;
            }
        }

        // ── Audio ─────────────────────────────────────────────────────────────
        while let Ok(Some(frame)) = mic.poll_frame() {
            if aenc.push_frame(&frame).is_ok() {
                drain_audio(aenc, &mut apackets);
            }
        }
    }

    venc.flush().expect("video flush");
    drain_video(venc, &mut vpackets);
    aenc.flush().ok();
    drain_audio(aenc, &mut apackets);
    (vpackets, apackets)
}

fn drain_video(enc: &mut dyn VideoEncoder, out: &mut Vec<mediaway_common::Packet>) {
    while let Some(p) = enc.poll_packet().expect("poll video packet") {
        out.push(p);
    }
}

fn drain_audio<E: AudioEncoder + ?Sized>(enc: &mut E, out: &mut Vec<mediaway_common::Packet>) {
    while let Some(p) = enc.poll_packet().expect("poll audio packet") {
        out.push(p);
    }
}

/// Mux the video and AAC audio tracks into one two-track fragmented MP4.
fn mux_tracks(
    venc: &dyn VideoEncoder,
    aenc: &WindowsAudioEncoder,
    mut vpackets: Vec<mediaway_common::Packet>,
    mut apackets: Vec<mediaway_common::Packet>,
) -> Vec<u8> {
    let mut open = Muxer::with_fragment_batch(2);
    let v_track = open
        .add_track(venc.stream_info().clone())
        .expect("add video track");
    let a_track = open
        .add_track(aenc.stream_info().clone())
        .expect("add audio track");

    let mut mux = open.begin();
    for p in &mut vpackets {
        p.stream_id = v_track;
        mux.push_packet(p).expect("mux video packet");
    }
    for p in &mut apackets {
        p.stream_id = a_track;
        mux.push_packet(p).expect("mux audio packet");
    }
    mux.flush();

    let mut bytes = Vec::new();
    mux.poll_bytes(&mut bytes);
    println!(
        "screen_record: video={} audio={} packets",
        vpackets.len(),
        apackets.len()
    );
    bytes
}
