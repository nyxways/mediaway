"""RC-stage binding check: round-trip synthetic packets through the
release-built mediaway_ffi.dll.

Mux 90 synthetic H.264 video + 90 synthetic AAC audio packets into a
fragmented MP4, demux the bytes back, and assert the 1:1 round-trip. This is
the same contract as examples/container/mux_roundtrip.py, packaged as an
assert-based test with no pytest dependency. Pure CPU: no hardware required.

Run from bindings/python (the DLL must be staged at
mediaway/_native/mediaway_ffi.dll):

    python tests/test_mux_roundtrip.py

A failed assertion raises AssertionError and exits nonzero, which is the RC
job's failure signal.
"""

import os
import sys

# Script mode puts tests/ on sys.path, not bindings/python. Bootstrap the
# path so `python tests/test_mux_roundtrip.py` imports the source package
# regardless of how the environment seeds sys.path.
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from mediaway import (
    AudioStreamInfo,
    Codec,
    Demuxer,
    Muxer,
    Packet,
    Rational,
    VideoStreamInfo,
)

VIDEO_FRAMES = 90  # synthetic video packets to mux
AUDIO_FRAMES = 90  # synthetic audio packets to mux
WIDTH = 640
HEIGHT = 480


def synthetic_video_packet(stream_index: int, i: int) -> Packet:
    """A fake H.264 access unit at 30 fps, so frame i has pts = i/30 s."""
    return Packet(
        stream_index=stream_index,
        pts=Rational(i, 30),
        payload=bytes([i & 0xFF]) * 2048,  # opaque encoded bitstream
        key=True,  # every synthetic frame is a sync sample
    )


def synthetic_audio_packet(stream_index: int, i: int) -> Packet:
    """A fake AAC frame: 1024 samples per frame at 48 kHz."""
    return Packet(
        stream_index=stream_index,
        pts=Rational(i * 1024, 48000),
        payload=bytes([(i * 7) & 0xFF]) * 512,
    )


def run_roundtrip() -> None:
    # --- mux ---------------------------------------------------------------
    with Muxer() as muxer:
        # Stream ids are assigned in registration order: video is 0, audio is 1.
        video_id = muxer.add_video_track(
            VideoStreamInfo(
                codec=Codec.H264,
                width=WIDTH,
                height=HEIGHT,
                frame_rate=Rational(1, 30),
            )
        )
        audio_id = muxer.add_audio_track(
            AudioStreamInfo(
                codec=Codec.AAC,
                sample_rate=48000,
                channels=2,
            )
        )

        # begin() is terminal for the Muxer: it moves the handle into the
        # LiveMuxer, which owns it (and frees it) from here on.
        with muxer.begin() as live:
            for i in range(VIDEO_FRAMES):
                live.push_packet(synthetic_video_packet(video_id, i))
            for i in range(AUDIO_FRAMES):
                live.push_packet(synthetic_audio_packet(audio_id, i))
            live.flush()

            chunks = []
            while True:
                chunk = live.poll_bytes()
                if chunk is None:
                    break
                chunks.append(chunk)

    mp4 = b"".join(chunks)
    assert len(mp4) > 0, "muxed output must not be empty"

    # --- demux -------------------------------------------------------------
    with Demuxer() as demuxer:
        demuxer.push_bytes(mp4)

        streams = demuxer.streams()
        assert len(streams) == 2, f"expected 2 streams, got {len(streams)}"

        video_stream = next(s for s in streams if isinstance(s, VideoStreamInfo))
        audio_stream = next(s for s in streams if isinstance(s, AudioStreamInfo))

        assert video_stream.codec == Codec.H264, "video codec must round-trip"
        assert (video_stream.width, video_stream.height) == (WIDTH, HEIGHT), (
            "video dimensions must round-trip"
        )
        assert video_stream.frame_rate == Rational(1, 30), (
            "video frame rate must round-trip"
        )
        # Synthetic AAC carries no AudioSpecificConfig, so the demuxer reports
        # only the codec for the audio track; assert codec, not rate/channels.
        assert audio_stream.codec == Codec.AAC, "audio codec must round-trip"

        video_count = 0
        audio_count = 0
        while True:
            packet = demuxer.poll_packet()
            if packet is None:
                break
            if packet.stream_index == video_id:
                video_count += 1
            elif packet.stream_index == audio_id:
                audio_count += 1

    assert video_count == VIDEO_FRAMES, "video packets must round-trip"
    assert audio_count == AUDIO_FRAMES, "audio packets must round-trip"


if __name__ == "__main__":
    run_roundtrip()
    print(
        f"PASS: {VIDEO_FRAMES} video + {AUDIO_FRAMES} audio packets "
        f"round-tripped through mediaway_ffi.dll"
    )
