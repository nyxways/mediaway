//! Integration: screen (DXGI Zero-Copy) + mic (WASAPI) capture through
//! `mediaway::platform`, encoded (H.264 DX11 Zero-Copy + AAC) and
//! muxed into one two-track fMP4 — proves the Stage 1 roadmap item
//! "Screen-record example composed through this crate end-to-end".
//!
//! Both tracks are composed through [`mediaway::EncodeSession`]:
//! [`EncodeSession::open_with_audio`] registers the video and audio encoders as MP4
//! tracks 0 and 1 on one shared `mp4::Muxer` before `begin()` (ADR-0003 — the
//! muxer's typestate means the audio track must be declared at construction),
//! the record loop feeds capture frames in via
//! `write_frame`/`write_audio_frame` (the session drains encoded packets into the
//! muxer on every push), and `finish()` flushes both encoders and returns the complete
//! two-track fMP4 bytes. This used to hand-roll that muxer composition
//! (`Muxer::with_fragment_batch` → `add_track` ×2 → `begin` → `push_packet`) — possible
//! through the session since ADR-0003, migrated per roadmap Stage 1b.
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

use mediaway::EncodeSession;
use mediaway::platform;
use mediaway_common::{
    CodecKind, GpuDeviceHandle, NativeHandle, PixelFormat, Rational, SampleFormat,
};
use mediaway_container::mp4::Demuxer;
use mediaway_device::Select;
use mediaway_device::audio::{AudioCapture, AudioCaptureConfig};
use mediaway_device::desktop::{
    CaptureOutputPreference, DesktopCaptureSource, DesktopVideoCapture, DesktopVideoCaptureConfig,
};
use mediaway_encoder::auto::{AutoVideoEncodeConfig, EncodePathClass};
use mediaway_encoder::windows::WindowsAudioEncoder;
use mediaway_encoder::{AudioEncoderConfig, VideoEncoder};
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
    let venc = match platform::AutoEncoder::open(&venc_cfg) {
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
    let aenc = match WindowsAudioEncoder::open(&aenc_cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: audio encoder unavailable ({e:?})");
            screen.close().ok();
            mic.close().ok();
            return;
        }
    };

    // ADR-0003: both tracks registered before the muxer's `begin()`, video 0 /
    // audio 1 — the session also flushes both encoders in `finish()`.
    let mut session = EncodeSession::open_with_audio(venc, aenc).expect("open_with_audio");

    let (video_frames, audio_frames) = record_loop(&mut *screen, &mut *mic, &mut session);

    screen.close().ok();
    mic.close().ok();

    if video_frames == 0 {
        eprintln!("skip: no video frames captured during the recording window");
        return;
    }
    assert!(
        audio_frames > 0,
        "expected at least one mic frame during the recording window"
    );

    let bytes = session.finish().expect("session finish");
    assert_two_track_fmp4(&bytes, video_frames, audio_frames);
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

/// Bounded capture→encode loop: polls screen + mic and pushes each captured
/// frame into the shared [`EncodeSession`], which encodes and muxes both
/// tracks internally. Returns the per-track count of frames successfully
/// pushed — the session is opaque for packet counts (packets drain straight
/// into its muxer), so the callers assert on frames instead. Terminates at
/// [`CAPTURE_SECS`] regardless of activity — not "until Ctrl+C".
fn record_loop<E: VideoEncoder>(
    screen: &mut dyn DesktopVideoCapture,
    mic: &mut dyn AudioCapture,
    session: &mut EncodeSession<E>,
) -> (usize, usize) {
    let mut video_frames = 0usize;
    let mut audio_frames = 0usize;
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
                let pushed = session.write_frame(&frame);
                // Release only after the session has consumed the texture
                // (write_frame's DX11 path drains synchronously) — matches
                // real hardware-encode pacing instead of releasing early.
                let _ = screen.release_frame();
                match pushed {
                    Ok(()) => video_frames += 1,
                    Err(e) => eprintln!("screen_mic_av_smoke: video write error ({e:?})"),
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("screen_mic_av_smoke: capture error ({e}), stopping capture loop");
                break;
            }
        }

        while let Ok(Some(frame)) = mic.poll_frame() {
            match session.write_audio_frame(&frame) {
                Ok(()) => audio_frames += 1,
                Err(e) => eprintln!("screen_mic_av_smoke: audio write error ({e:?})"),
            }
        }

        std::thread::sleep(TICK);
    }

    // SAFETY: restore the cursor to where the loop found it.
    unsafe {
        let _ = SetCursorPos(origin.x, origin.y);
    }
    (video_frames, audio_frames)
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

/// Write the finished two-track fMP4 to a temp file, demux it back, and
/// assert real output properties: non-trivial size, exactly 2 tracks, and
/// per-track demuxed packet counts consistent with what was pushed through
/// the session. The session drains encoded packets into its own muxer, so the
/// test can no longer observe packet counts: the video bound is exact (the
/// WMF H.264 encoder emits one packet per input frame — 1:1, flushed by
/// `finish()`), the audio bound is existence (each AAC packet consumes ~2.13
/// 480-sample capture frames, so no honest 1:1 frame→packet bound exists).
fn assert_two_track_fmp4(bytes: &[u8], video_frames: usize, audio_frames: usize) {
    assert!(
        bytes.len() > 1_000,
        "fmp4 output implausibly small: {} bytes",
        bytes.len()
    );
    assert_eq!(&bytes[4..8], b"ftyp");

    let path = std::env::temp_dir().join("mediaway_screen_mic_av_smoke.mp4");
    std::fs::write(&path, bytes).expect("write fmp4 to disk");
    let written_len = std::fs::metadata(&path).expect("stat written fmp4").len();
    assert!(written_len > 1_000, "written file implausibly small");

    let mut demux = Demuxer::new();
    demux.push_bytes(bytes);
    assert_eq!(
        demux.streams().len(),
        2,
        "expected exactly 2 demuxed tracks"
    );

    // The session muxed video as track 0 and audio as track 1 (ADR-0003
    // renumbers explicitly), so demuxed packets carry those ids back out.
    let mut demuxed_video = 0usize;
    let mut demuxed_audio = 0usize;
    while let Some(p) = demux.poll_packet() {
        match p.stream_id {
            0 => demuxed_video += 1,
            1 => demuxed_audio += 1,
            _ => {}
        }
    }
    assert!(
        demuxed_video >= video_frames,
        "demuxed {demuxed_video} video packets, expected >= {video_frames} (video frames pushed)"
    );
    assert!(
        demuxed_audio >= 1,
        "expected at least one demuxed audio packet (audio frames pushed: {audio_frames})"
    );

    std::fs::remove_file(&path).ok();

    eprintln!(
        "screen_and_mic_to_fmp4_two_tracks: video_frames={} audio_frames={} demuxed_video={} demuxed_audio={} bytes={}",
        video_frames,
        audio_frames,
        demuxed_video,
        demuxed_audio,
        bytes.len()
    );
}
