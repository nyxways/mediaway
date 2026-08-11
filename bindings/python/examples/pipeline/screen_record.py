"""Screen + mic -> H.264 encode -> MP4 (out_screen.mp4).

Real Zero-Copy Screen capture + the capture-to-encode bridge: a `GpuDevice`
(adr/0007-gpu-device-factory.md) drives real Screen capture, and
`EncodeSession.write_frame_from_desktop_capture` (adr/pipeline/0005) polls +
pushes each frame in one native call — no intermediate `VideoFrame`, Zero-Copy
for Screen's GPU-backed frames. `AutoVideoEncoder.pick(..., pixel_format=BGRA8,
gpu_device=gpu)` negotiates the GPU-input-capable path before the bridge is
ever called (DXGI delivers BGRA8, not the NV12 default).

On a machine whose encoder backend accepts a GPU-configured open but rejects
the GPU-input path once frames actually start flowing (a real, pre-existing
WMF/DX11 limitation — see the Rust `gpu_write_frame_smoke.rs` test and the
C/Node.js/C# `screen_record` siblings), this gracefully skips instead of
crashing.

The audio side is not wired into the output MP4 as a second track yet — mic
PCM is drained (proves capture works), not muxed, same as `ScreenRecord.cs`'s
C# equivalent. `camera_record.py`'s two-track remux is a separate flow this
file doesn't duplicate.
"""

from contextlib import nullcontext
from pathlib import Path
import time

from mediaway import (
    AudioCapture,
    AutoVideoEncoder,
    CaptureUnsupportedError,
    DeviceUnavailableError,
    EncodeSession,
    GpuDevice,
    MediawayError,
    PixelFormat,
    Rational,
    VideoCapture,
)

RECORD_SECONDS = 3.0


def fmt_rational(r: Rational) -> str:
    """Render a Rational as a compact "num/den" string."""
    return f"{r.num}/{r.den}"


def main() -> None:
    try:
        gpu = GpuDevice.create(video_support=True)
    except (CaptureUnsupportedError, DeviceUnavailableError) as err:
        print(f"GPU device unavailable: {err} — exiting.")
        return

    with gpu:
        try:
            screen = VideoCapture.open("screen", index=0, frame_rate=Rational(1, 30), gpu_device=gpu)
        except (CaptureUnsupportedError, DeviceUnavailableError) as err:
            print(f"screen capture unavailable: {err} — exiting.")
            return

        with screen:
            width, height = screen.size()
            fps = screen.frame_rate()
            print(f"screen geometry: {width}x{height} @ {fmt_rational(fps)}")

            try:
                encoder = AutoVideoEncoder.pick(
                    width=width,
                    height=height,
                    frame_rate=fps,
                    pixel_format=PixelFormat.BGRA8,
                    bitrate_bps=8_000_000,
                    gpu_device=gpu,
                )
            except MediawayError as err:
                print(f"no GPU-input encoder available for {width}x{height}: {err} — exiting.")
                return
            print(f"picked encoder: {encoder.name} ({encoder.codec.name})")

            mic = None
            try:
                mic = AudioCapture.open("mic", 48000)
                print(f"mic open: {mic.sample_rate()} Hz, {mic.channels()} ch")
            except DeviceUnavailableError as err:
                print(f"mic unavailable ({err}); recording video only")

            with mic or nullcontext(), EncodeSession(encoder) as session:
                deadline = time.monotonic() + RECORD_SECONDS
                frames_written = 0
                pcm_bytes = 0
                try:
                    while time.monotonic() < deadline:
                        if session.write_frame_from_desktop_capture(screen):
                            frames_written += 1
                        else:
                            time.sleep(0.004)

                        if mic is not None:
                            pcm = mic.poll_pcm(timeout=0.0)
                            if pcm is not None:
                                pcm_bytes += len(pcm)
                except MediawayError as err:
                    print(f"GPU-input encode unsupported on this backend ({err}) — exiting.")
                    return

                if frames_written == 0:
                    print("screen capture opened but delivered no frames within the deadline — exiting.")
                    return

                mp4 = session.finish()  # terminal: consumes the session

    out_path = Path("out_screen.mp4")
    out_path.write_bytes(mp4)
    print(
        f"bridged {frames_written} real screen frame(s) over {RECORD_SECONDS:.0f} s "
        f"(drained {pcm_bytes} PCM bytes) -> {out_path} ({len(mp4)} bytes)"
    )


if __name__ == "__main__":
    main()
