"""Camera + mic capture -> encode -> fragmented MP4 — aspirational quick-start example.

ASPIRATIONAL EXAMPLE: no `mediaway` Python package exists yet. This file shows
the target ergonomics for a future Python binding over Mediaway's C ABI
(ctypes/cffi under the hood, wrapped in idiomatic Python: context managers,
exceptions, snake_case). See ../README.md and docs/spec/c-ffi.md.

Same shape as screen_record.py, with a camera source instead of a screen
source: open a camera capture + microphone (both fallible — the specific
device may not be available), build an "auto video encode" config at the
capture's real geometry, and reuse the exact same building blocks as
encode_to_mp4.py (AutoEncoder -> EncodeSession -> write_frame -> finish) plus
the one small platform-agnostic `record()` loop that glues capture to encode.
`record()` is typed purely against the `VideoCapture` / `AudioCapture`
abstract base classes, so it does not know or care that a camera (rather than
a screen) is underneath — the exact same function works unchanged for either.

Run (once the real package exists):
    python camera_record.py
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from pathlib import Path
from time import monotonic

from mediaway import (
    AutoEncoder,
    AutoVideoEncodeConfig,
    CameraCapture,
    CaptureUnavailableError,
    CodecKind,
    EncodeSession,
    Microphone,
    NoEncoderAvailableError,
    PixelFormat,
    Rational,
    VideoFrame,
)

FPS = 30
SECONDS = 3.0
BITRATE_BPS = 4_000_000


# ── Platform-agnostic capture contracts ─────────────────────────────────────
#
# `record()` below only ever sees these two ABCs. `CameraCapture` and
# `Microphone` (imported above) are concrete backends that implement them;
# swapping in another source (e.g. screen capture) requires no change to
# `record()`.


class VideoCapture(ABC):
    """Abstract video capture source (camera, screen, window, ...)."""

    @property
    @abstractmethod
    def width(self) -> int:
        """Actual stream width the backend settled on, once opened."""

    @property
    @abstractmethod
    def height(self) -> int:
        """Actual stream height the backend settled on, once opened."""

    @abstractmethod
    def poll_frame(self) -> object | None:
        """Non-blocking poll: a new frame, `None` if nothing is ready yet.

        Raises on a hard capture error. The returned frame may reference
        GPU-resident memory that must be released via `release_frame()` once
        the caller is done with it.
        """

    @abstractmethod
    def release_frame(self) -> None:
        """Release the most recently polled frame back to the OS."""

    @abstractmethod
    def close(self) -> None: ...

    def __enter__(self) -> VideoCapture:
        return self

    def __exit__(self, *exc_info: object) -> None:
        self.close()


class AudioCapture(ABC):
    """Abstract audio capture source (microphone, loopback, ...)."""

    @abstractmethod
    def poll_frame(self) -> object | None:
        """Non-blocking poll: a new frame, `None` if nothing is ready yet.

        Raises on a hard capture error.
        """

    @abstractmethod
    def close(self) -> None: ...

    def __enter__(self) -> AudioCapture:
        return self

    def __exit__(self, *exc_info: object) -> None:
        self.close()


def make_grey_nv12(width: int, height: int) -> bytes:
    """Build one solid-grey NV12 frame: Y=128 everywhere, U/V=128 everywhere.

    Layout is `width * height` Y bytes followed by `width * height / 2`
    interleaved UV bytes. Stand-in for a real captured-frame conversion.
    """
    y_plane = bytes([128]) * (width * height)
    uv_plane = bytes([128]) * (width * height // 2)
    return y_plane + uv_plane


def record(
    video: VideoCapture,
    audio: AudioCapture | None,
    session: EncodeSession,
    duration_seconds: float,
) -> None:
    """Poll `video` (and drain `audio`, if present) until `duration_seconds` elapses.

    Writes a synthetic grey NV12 placeholder frame into `session` for every
    polled video frame — this example never touches the real captured pixels.
    Typed purely against the `VideoCapture` / `AudioCapture` ABCs: this
    function has no idea which concrete source (camera, screen, ...) it was
    handed.
    """
    deadline = monotonic() + duration_seconds
    frame_data = make_grey_nv12(video.width, video.height)
    pts = 0

    while monotonic() < deadline:
        if video.poll_frame() is not None:
            # Real backends: this frame may be a GPU handle. We never touch
            # the pixels here, but the OS-side reference must still be
            # released explicitly once we're done with it.
            video.release_frame()

            session.write_frame(
                VideoFrame(
                    pts=pts,
                    duration=1,
                    width=video.width,
                    height=video.height,
                    pixel_format=PixelFormat.NV12,
                    data=frame_data,
                )
            )
            pts += 1

        if audio is not None:
            # Drain polled audio frames; not wired to an audio track yet.
            while audio.poll_frame() is not None:
                pass


def main() -> None:
    frame_rate = Rational(1, FPS)

    try:
        cap = CameraCapture.open(device_index=0, frame_rate=frame_rate)
    except CaptureUnavailableError as e:
        print(f"camera_record: camera unavailable ({e}) -- device not available")
        return

    mic: AudioCapture | None
    try:
        mic = Microphone.open(sample_rate=Rational(1, 48_000))
    except CaptureUnavailableError as e:
        print(f"camera_record: mic unavailable ({e}) -- continuing without audio")
        mic = None

    print(f"camera_record: {cap.width}x{cap.height} camera" + (", mic ready" if mic else ""))

    config = AutoVideoEncodeConfig(
        codec=CodecKind.H264,
        width=cap.width,
        height=cap.height,
        frame_rate=frame_rate,
    )
    config.bitrate_bps = BITRATE_BPS

    try:
        encoder = AutoEncoder.open(config)
    except NoEncoderAvailableError as e:
        print(f"camera_record: encoder unavailable ({e})")
        cap.close()
        if mic is not None:
            mic.close()
        return

    with EncodeSession.open(encoder) as session:
        record(cap, mic, session, duration_seconds=SECONDS)
        cap.close()
        if mic is not None:
            mic.close()
        mp4_bytes = session.finish()

    Path("out_camera.mp4").write_bytes(mp4_bytes)
    print(f"camera_record: -> out_camera.mp4 ({len(mp4_bytes)} bytes)")


if __name__ == "__main__":
    main()
