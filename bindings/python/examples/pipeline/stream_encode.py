"""Encode 300 synthetic NV12 frames and stream the fragmented MP4 to a file as it is
produced, instead of holding the whole recording in memory until `finish()`.

✅ REAL — `EncodeSession.poll_bytes()` (adr/pipeline/0007 §1) returns whatever fMP4
bytes are ready now. Call it after every frame and write the chunk out; the session's
memory then stays bounded by the poll cadence, not by the recording's length. A
long capture that is never polled holds the entire file in RAM until `finish()`.

`finish()` after polling returns only the **unpolled tail** — write it after the last
chunk. Concatenated in order, the chunks and the tail are the complete file.
"""

from pathlib import Path

from mediaway import (
    AutoVideoEncoder,
    Codec,
    Demuxer,
    EncodeSession,
    EncoderUnavailableError,
    PixelFormat,
    Rational,
    VideoFrame,
)

WIDTH = 640
HEIGHT = 480
FRAMES = 300


def main() -> None:
    try:
        encoder = AutoVideoEncoder.pick(codec=Codec.H264, width=WIDTH, height=HEIGHT, frame_rate=Rational(1, 30))
    except EncoderUnavailableError as err:
        print(f"no H.264 encoder available: {err}")
        return

    grey = bytes([0x80]) * (WIDTH * HEIGHT + WIDTH * HEIGHT // 2)
    out_path = Path("stream_out.mp4")
    chunks = 0
    largest = 0
    with out_path.open("wb") as out, EncodeSession(encoder) as session:
        for i in range(FRAMES):
            session.push_frame(
                VideoFrame(width=WIDTH, height=HEIGHT, format=PixelFormat.NV12, data=grey, pts=Rational(i, 30))
            )
            chunk = session.poll_bytes()  # b"" when nothing is ready yet
            if chunk:
                out.write(chunk)
                chunks += 1
                largest = max(largest, len(chunk))
        tail = session.finish()  # terminal; only what was not polled
        out.write(tail)

    total = out_path.stat().st_size
    print(f"streamed {FRAMES} frames -> {out_path}: {chunks} chunks (largest {largest} B) + {len(tail)} B tail = {total} B")

    # Prove the concatenation is one valid file: demux it and count the packets.
    with Demuxer() as demuxer:
        demuxer.push_bytes(out_path.read_bytes())
        packets = 0
        while demuxer.poll_packet() is not None:
            packets += 1
    print(f"demuxed {packets} packets from the streamed file")
    assert packets == FRAMES, f"expected {FRAMES} packets, got {packets}"


if __name__ == "__main__":
    main()
