"""Auto video encode -> fragmented MP4 — aspirational quick-start example.

ASPIRATIONAL EXAMPLE: no `mediaway` Python package exists yet. This file shows
the target ergonomics for a future Python binding over Mediaway's C ABI
(ctypes/cffi under the hood, wrapped in idiomatic Python: context managers,
exceptions, snake_case). See ../README.md and docs/spec/c-ffi.md.

Mirrors examples/encode_to_mp4.rs: pick the best available OS/GPU H.264
encoder (Zero-Copy GPU path preferred, CPU-upload fallback), feed it 3 seconds
of a synthetic grey NV12 clip, and mux the resulting packets into a
fragmented MP4 via the built-in high-level pipeline — the caller just pushes
frames and gets MP4 bytes back.

Run (once the real package exists):
    python encode_to_mp4.py
"""

from __future__ import annotations

from pathlib import Path

from mediaway import (
    AutoEncoder,
    AutoVideoEncodeConfig,
    CodecKind,
    EncodeSession,
    NoEncoderAvailableError,
    PixelFormat,
    Rational,
    VideoFrame,
)

WIDTH = 640
HEIGHT = 480
FPS = 30
SECONDS = 3
BITRATE_BPS = 2_000_000


def make_grey_nv12(width: int, height: int) -> bytes:
    """Build one solid-grey NV12 frame: Y=128 everywhere, U/V=128 everywhere.

    Layout is `width * height` Y bytes followed by `width * height / 2`
    interleaved UV bytes.
    """
    y_plane = bytes([128]) * (width * height)
    uv_plane = bytes([128]) * (width * height // 2)
    return y_plane + uv_plane


def encode_grey_clip() -> bytes | None:
    """Encode `SECONDS` of a synthetic grey clip to fMP4 bytes.

    Returns None if this platform has no suitable H.264 encoder backend yet.
    """
    # Defaults for H.264 at this resolution/framerate, then override bitrate.
    config = AutoVideoEncodeConfig(
        codec=CodecKind.H264,
        width=WIDTH,
        height=HEIGHT,
        frame_rate=Rational(1, FPS),
    )
    config.bitrate_bps = BITRATE_BPS

    try:
        encoder = AutoEncoder.open(config)
    except NoEncoderAvailableError as e:
        print(f"encode_to_mp4: no encoder available on this platform ({e})")
        return None

    print("encode_to_mp4: running on this platform")
    frame_data = make_grey_nv12(WIDTH, HEIGHT)

    with EncodeSession.open(encoder) as session:
        for pts in range(FPS * SECONDS):
            session.write_frame(
                VideoFrame(
                    pts=pts,
                    duration=1,
                    width=WIDTH,
                    height=HEIGHT,
                    pixel_format=PixelFormat.NV12,
                    data=frame_data,
                )
            )
        return session.finish()


if __name__ == "__main__":
    mp4_bytes = encode_grey_clip()
    if mp4_bytes is None:
        raise SystemExit(0)

    Path("out.mp4").write_bytes(mp4_bytes)

    n_frames = FPS * SECONDS
    print(f"encode_to_mp4: {n_frames} frames -> out.mp4 ({len(mp4_bytes)} bytes)")
