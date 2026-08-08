"""Mux 90 synthetic video + 90 synthetic audio packets into a fragmented MP4,
then demux the result and count what comes back.

✅ REAL — the container mux/demux capability is fully implemented in the
native C ABI (`mediaway_ffi`); this example runs against it. Only
the packets themselves are synthetic.

The muxer is sans-io: it never touches the filesystem. The caller owns byte
I/O — we accumulate `poll_bytes()` chunks and hand them to the demuxer.
"""

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


def fmt_rational(r: Rational) -> str:
    """Render a Rational as a compact "num/den" string."""
    return f"{r.num}/{r.den}"


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


def main() -> None:
    # --- mux ---------------------------------------------------------------
    with Muxer() as muxer:
        # Stream ids are assigned in registration order starting at 1: video is 1, audio is 2.
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
        # LiveMuxer, which owns it (and frees it) from here on. Track
        # registration on a LiveMuxer is impossible, matching the ABI's
        # INVALID_STATE.
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
    print(
        f"muxed {VIDEO_FRAMES} video + {AUDIO_FRAMES} audio packets "
        f"-> {len(mp4)} bytes"
    )

    # --- demux -------------------------------------------------------------
    with Demuxer() as demuxer:
        demuxer.push_bytes(mp4)

        streams = demuxer.streams()
        print(f"recovered {len(streams)} stream(s):")
        for stream in streams:
            if isinstance(stream, VideoStreamInfo):
                print(
                    f"  video: {stream.codec.name} "
                    f"{stream.width}x{stream.height} @ "
                    f"{fmt_rational(stream.frame_rate)}"
                )
            elif isinstance(stream, AudioStreamInfo):
                print(
                    f"  audio: {stream.codec.name} "
                    f"{stream.sample_rate} Hz, {stream.channels} ch"
                )

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

    print(f"recovered {video_count} video packets, {audio_count} audio packets")
    assert video_count == VIDEO_FRAMES, "video packets must roundtrip"
    assert audio_count == AUDIO_FRAMES, "audio packets must roundtrip"


if __name__ == "__main__":
    main()
