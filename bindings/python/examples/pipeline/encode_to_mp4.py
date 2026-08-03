"""Auto-pick the best available H.264 encoder, push 90 synthetic grey NV12
frames through it, and write the resulting fragmented MP4 to out.mp4.

✅ REAL — the auto video encode -> fMP4 pipeline is implemented in the native
C ABI (`mediaway_pipeline_ffi`); this example runs against it. Only the frames
themselves are synthetic (flat grey, NV12).

The encoder is chosen for us by AutoVideoEncoder.pick(). EncodeSession takes
ownership of that choice, and finish() is terminal: it returns the complete
MP4 bytes and consumes the session (no close() afterwards — __exit__ on an
already-finished session is a no-op).
"""

from pathlib import Path

from mediaway import (
    AutoVideoEncoder,
    Codec,
    EncodeSession,
    EncoderUnavailableError,
    PixelFormat,
    Rational,
    VideoFrame,
)

WIDTH = 640
HEIGHT = 480
FRAMES = 90


def make_grey_nv12_frame(i: int) -> VideoFrame:
    """A flat-grey NV12 frame (Y = U = V = 0x80) with presentation time i/30 s."""
    y_plane = WIDTH * HEIGHT
    uv_plane = WIDTH * HEIGHT // 2
    return VideoFrame(
        width=WIDTH,
        height=HEIGHT,
        format=PixelFormat.NV12,
        data=bytes([0x80]) * (y_plane + uv_plane),
        pts=Rational(i, 30),
    )


def main() -> None:
    try:
        encoder = AutoVideoEncoder.pick(
            codec=Codec.H264,
            width=WIDTH,
            height=HEIGHT,
            frame_rate=Rational(1, 30),
        )
    except EncoderUnavailableError as err:
        print(f"no H.264 encoder available: {err}")
        return
    print(f"picked encoder: {encoder.name} ({encoder.codec.name})")

    with EncodeSession(encoder) as session:
        for i in range(FRAMES):
            session.push_frame(make_grey_nv12_frame(i))
        mp4 = session.finish()  # terminal: consumes the session

    out_path = Path("out.mp4")
    out_path.write_bytes(mp4)
    print(f"encoded {FRAMES} frames -> {out_path} ({len(mp4)} bytes)")


if __name__ == "__main__":
    main()
