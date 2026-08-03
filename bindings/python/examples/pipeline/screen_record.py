"""Screen + mic -> H.264 encode -> MP4 (out.mp4).

🚧 aspirational — ABI returns UNSUPPORTED for screen today. Screen capture
needs a live GPU device handle (`ID3D11Device*`) with no CPU fallback, and its
C representation is deferred, so `VideoCapture.open("screen", ...)` raises
CaptureUnsupportedError against the current ABI. This file shows the ideal
flow the DX contract targets once that gap is closed.

The audio side is aspirational too: no audio encoder exists in the ABI, so mic
PCM is drained rather than muxed — the same documented gap as camera_record.py.
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
    EncoderUnavailableError,
    Rational,
    VideoCapture,
)

RECORD_SECONDS = 5.0


def fmt_rational(r: Rational) -> str:
    """Render a Rational as a compact "num/den" string."""
    return f"{r.num}/{r.den}"


def main() -> None:
    # Against the current ABI this raises CaptureUnsupportedError (the native
    # status UNSUPPORTED is an expected outcome, not an error). The guard
    # keeps the example from crashing on real hardware today.
    try:
        screen = VideoCapture.open("screen", index=0, frame_rate=Rational(1, 30))
    except CaptureUnsupportedError as err:
        print(f"screen capture is UNSUPPORTED in the native ABI: {err}")
        print("(aspirational example — see the header comment)")
        return

    with screen:
        width, height = screen.size()
        fps = screen.frame_rate()
        print(f"screen geometry: {width}x{height} @ {fmt_rational(fps)}")

        try:
            encoder = AutoVideoEncoder.pick(
                width=width, height=height, frame_rate=fps
            )
        except EncoderUnavailableError as err:
            print(f"no encoder available for {width}x{height}: {err}")
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
            frames_recorded = 0
            pcm_bytes = 0
            while time.monotonic() < deadline:
                frame = screen.poll_frame(timeout=0.1)
                if frame is not None:
                    session.push_frame(frame)
                    frames_recorded += 1

                if mic is not None:
                    pcm = mic.poll_pcm(timeout=0.0)
                    if pcm is not None:
                        pcm_bytes += len(pcm)

            # Audio note: the screen path is blocked before audio matters —
            # Screen capture needs a live GPU device handle from C
            # (mediaway-ffi/adr/0001, § Deferred), so the mic PCM is
            # drained, not muxed.
            mp4 = session.finish()  # terminal: consumes the session

    out_path = Path("out.mp4")
    out_path.write_bytes(mp4)
    print(
        f"recorded {frames_recorded} frames over {RECORD_SECONDS:.0f} s "
        f"(drained {pcm_bytes} PCM bytes) -> {out_path} ({len(mp4)} bytes)"
    )


if __name__ == "__main__":
    main()
