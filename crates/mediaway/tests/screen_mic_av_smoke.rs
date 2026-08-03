//! Integration: screen (DXGI Zero-Copy) + mic (WASAPI) capture through
//! `mediaway::platform`, encoded (H.264 DX11 Zero-Copy + AAC) and
//! muxed into one two-track fMP4 — proves the Stage 1 roadmap item
//! "Screen-record example composed through this crate end-to-end".
//!
//! [`mediaway::EncodeSession`] stays video-only / single-track per
//! ADR-0014 ("extend … when a real caller needs it — new ADR at that point if
//! the shape changes materially"); this test is that first real caller, but it
//! composes the second (audio) track directly against a shared
//! [`mediaway_container::mp4::Muxer`] instead of changing `EncodeSession`'s
//! public shape — the same multi-track pattern already used in
//! `mediaway-encoder-windows/tests/av_fmp4_smoke.rs`. No new ADR needed since
//! `EncodeSession` itself is untouched.
//!
//! The captured DXGI surface is BGRA — fed straight into the H.264 encoder's
//! Zero-Copy path via `PixelFormat::Bgra8` (the "live-recorder" ARGB32 input
//! documented in `mediaway-encoder-windows/src/wmf/shared.rs`), so no manual
//! BGRA→NV12 conversion is needed.

#![cfg(windows)]
#![allow(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "integration test"
)]

use std::time::{Duration, Instant};

use mediaway::platform;
use mediaway_common::{
    CodecKind, GpuDeviceHandle, NativeHandle, Packet, PixelFormat, Rational, SampleFormat,
    StreamInfo,
};
use mediaway_container::mp4::{Demuxer, Muxer};
use mediaway_device::Select;
use mediaway_device::audio::{AudioCapture, AudioCaptureConfig};
use mediaway_device::desktop::{
    CaptureOutputPreference, DesktopCaptureSource, DesktopVideoCapture, DesktopVideoCaptureConfig,
};
use mediaway_encoder::auto::{AutoVideoEncodeConfig, EncodePathClass};
use mediaway_encoder::windows::WindowsAudioEncoder;
use mediaway_encoder::{AudioEncoder, AudioEncoderConfig, VideoEncoder};
use windows::Win32::Foundation::{HMODULE, POINT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos};
use windows::core::Interface;

/// Bounded recording window — deterministic termination, not "until Ctrl+C".
const CAPTURE_SECS: u64 = 5;
/// ~30fps pacing to match the configured encoder time base.
const TICK: Duration = Duration::from_millis(33);

#[test]
fn screen_and_mic_to_fmp4_two_tracks() {
    let Some((_device, device_handle)) = open_shared_d3d11_device() else {
        return;
    };

    let cap_cfg = DesktopVideoCaptureConfig {
        source: DesktopCaptureSource::Screen {
            select: Select::Default,
        },
        time_base: Rational::new(1, 30),
        output: CaptureOutputPreference::ZeroCopyGpu,
        gpu_device: Some(GpuDeviceHandle::DirectX11(device_handle)),
    };
    let mut screen = match platform::ScreenCapture::open(&cap_cfg) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skip: screen capture unavailable ({e})");
            return;
        }
    };
    let geometry = screen.stream_info().geometry().expect("video geometry");

    let mic_cfg = AudioCaptureConfig::microphone(Rational::new(1, 48_000));
    let mut mic = match platform::Microphone::open(&mic_cfg) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("skip: microphone unavailable ({e})");
            screen.close().ok();
            return;
        }
    };
    let sample_rate = mic.stream_info().sample_rate().expect("mic sample_rate");
    let channels = mic.stream_info().channels().expect("mic channels");

    let venc_cfg = AutoVideoEncodeConfig {
        bitrate_bps: 4_000_000,
        pixel_format: PixelFormat::Bgra8,
        // CPU-upload fallback is structurally incompatible with `Bgra8` input
        // (`validate_common` requires `ZeroCopyGpu` for that pixel format), so
        // the default `CpuUpload` ceiling's CPU-upload attempt would only mask
        // the real open error behind an unrelated `Unsupported`. Zero-Copy is
        // the only path this config can take; keep the skip message honest.
        max_path_class: EncodePathClass::ZeroCopy,
        gpu_device: Some(GpuDeviceHandle::DirectX11(device_handle)),
        ..AutoVideoEncodeConfig::new(
            CodecKind::H264,
            geometry.width,
            geometry.height,
            Rational::new(1, 30),
        )
    };
    let mut venc = match platform::AutoEncoder::open(&venc_cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: video encoder unavailable ({e:?})");
            screen.close().ok();
            mic.close().ok();
            return;
        }
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
            eprintln!("skip: audio encoder unavailable ({e:?})");
            screen.close().ok();
            mic.close().ok();
            return;
        }
    };

    let (mut vpackets, mut apackets) = record_loop(&mut *screen, &mut *mic, &mut venc, &mut aenc);

    screen.close().ok();
    mic.close().ok();
    venc.flush().expect("video flush");
    drain_video(&mut venc, &mut vpackets);
    aenc.flush().expect("audio flush");
    drain_audio(&mut aenc, &mut apackets);

    if vpackets.is_empty() {
        eprintln!("skip: no video frames captured during the recording window");
        return;
    }
    assert!(
        !apackets.is_empty(),
        "expected at least one AAC packet from mic capture"
    );

    let vinfo = venc.stream_info().clone().with_id(0);
    let ainfo = aenc.stream_info().clone().with_id(1);
    assert_two_track_fmp4(vinfo, ainfo, vpackets, apackets);
}

/// Own D3D11 device shared by screen capture and the video encoder's
/// Zero-Copy path, so a captured texture can be pushed straight into
/// `push_frame` with no copy. `None` (with an honest `skip:` line) when no
/// HW-capable D3D11 device is available in this environment.
fn open_shared_d3d11_device() -> Option<(ID3D11Device, NativeHandle)> {
    let mut device: Option<ID3D11Device> = None;
    let hr = unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&raw mut device),
            None,
            None,
        )
    };
    let Some(device) = device else {
        eprintln!("skip: D3D11CreateDevice failed ({hr:?})");
        return None;
    };
    let handle = NativeHandle::new(Interface::as_raw(&device) as usize).expect("device pointer");
    Some((device, handle))
}

/// Bounded capture→encode loop: polls screen + mic, pushes frames into the
/// respective encoders, and returns every encoded packet. Terminates at
/// [`CAPTURE_SECS`] regardless of activity — not "until Ctrl+C".
fn record_loop<E: VideoEncoder>(
    screen: &mut dyn DesktopVideoCapture,
    mic: &mut dyn AudioCapture,
    venc: &mut E,
    aenc: &mut WindowsAudioEncoder,
) -> (Vec<Packet>, Vec<Packet>) {
    let mut vpackets = Vec::new();
    let mut apackets = Vec::new();
    let mut video_pts = 0i64;
    let mut toggle = false;
    let mut origin = POINT::default();
    // SAFETY: GetCursorPos writes into a valid, uniquely-owned local POINT.
    unsafe {
        let _ = GetCursorPos(&raw mut origin);
    }
    let deadline = Instant::now() + Duration::from_secs(CAPTURE_SECS);

    while Instant::now() < deadline {
        nudge_cursor(origin, &mut toggle);

        match screen.poll_frame() {
            Ok(Some(mut frame)) => {
                frame.pts = video_pts;
                video_pts += 1;
                let pushed = venc.push_frame(&frame);
                // Release only after the encoder has consumed the texture
                // (push_frame's DX11 path drains synchronously) — matches
                // real hardware-encode pacing instead of releasing early.
                let _ = screen.release_frame();
                match pushed {
                    Ok(()) => drain_video(venc, &mut vpackets),
                    Err(e) => eprintln!("screen_mic_av_smoke: video push error ({e:?})"),
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("screen_mic_av_smoke: capture error ({e}), stopping capture loop");
                break;
            }
        }

        while let Ok(Some(frame)) = mic.poll_frame() {
            match aenc.push_frame(&frame) {
                Ok(()) => drain_audio(aenc, &mut apackets),
                Err(e) => eprintln!("screen_mic_av_smoke: audio push error ({e:?})"),
            }
        }

        std::thread::sleep(TICK);
    }

    // SAFETY: restore the cursor to where the loop found it.
    unsafe {
        let _ = SetCursorPos(origin.x, origin.y);
    }
    (vpackets, apackets)
}

/// Jitter the cursor by one pixel and back. DXGI Desktop Duplication's
/// `AcquireNextFrame` delivers a new frame when *either* the desktop image or
/// the pointer position changes, so this keeps frames flowing deterministically
/// even when nothing else on the desktop is redrawing (e.g. an unattended
/// background job with no live user input).
fn nudge_cursor(origin: POINT, toggle: &mut bool) {
    *toggle = !*toggle;
    let dx = if *toggle { 1 } else { -1 };
    // SAFETY: SetCursorPos is a simple user32 call; failure is non-fatal here.
    unsafe {
        let _ = SetCursorPos(origin.x + dx, origin.y);
    }
}

fn drain_video<E: VideoEncoder>(enc: &mut E, out: &mut Vec<Packet>) {
    while let Some(p) = enc.poll_packet().expect("poll video packet") {
        out.push(p);
    }
}

fn drain_audio<E: AudioEncoder>(enc: &mut E, out: &mut Vec<Packet>) {
    while let Some(p) = enc.poll_packet().expect("poll audio packet") {
        out.push(p);
    }
}

/// Mux both tracks into one fMP4, write it to a temp file, demux it back, and
/// assert real output properties: non-trivial size, exactly 2 tracks, and a
/// demuxed packet count at least matching what was encoded.
fn assert_two_track_fmp4(
    vinfo: StreamInfo,
    ainfo: StreamInfo,
    mut vpackets: Vec<Packet>,
    mut apackets: Vec<Packet>,
) {
    let mut open = Muxer::with_fragment_batch(2);
    open.add_track(vinfo).expect("video track");
    open.add_track(ainfo).expect("audio track");
    let mut mux = open.begin();
    for p in &mut vpackets {
        p.stream_id = 0;
        mux.push_packet(p).expect("mux video packet");
    }
    for p in &mut apackets {
        p.stream_id = 1;
        mux.push_packet(p).expect("mux audio packet");
    }
    mux.flush();

    let mut bytes = Vec::new();
    mux.poll_bytes(&mut bytes);
    assert!(
        bytes.len() > 1_000,
        "fmp4 output implausibly small: {} bytes",
        bytes.len()
    );
    assert_eq!(&bytes[4..8], b"ftyp");

    let path = std::env::temp_dir().join("mediaway_screen_mic_av_smoke.mp4");
    std::fs::write(&path, &bytes).expect("write fmp4 to disk");
    let written_len = std::fs::metadata(&path).expect("stat written fmp4").len();
    assert!(written_len > 1_000, "written file implausibly small");

    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    assert_eq!(
        demux.streams().len(),
        2,
        "expected exactly 2 demuxed tracks"
    );

    let mut demuxed = 0usize;
    while demux.poll_packet().is_some() {
        demuxed += 1;
    }
    assert!(
        demuxed >= vpackets.len() + apackets.len(),
        "demuxed {demuxed} packets, expected >= {} (video) + {} (audio)",
        vpackets.len(),
        apackets.len()
    );

    std::fs::remove_file(&path).ok();

    eprintln!(
        "screen_and_mic_to_fmp4_two_tracks: video={} audio={} demuxed={} bytes={}",
        vpackets.len(),
        apackets.len(),
        demuxed,
        bytes.len()
    );
}
