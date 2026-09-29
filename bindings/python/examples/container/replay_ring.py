"""Keep the last few seconds of packets in a replay ring, cut a clip at a keyframe,
write it as a standalone MP4, and demux that to prove it.

✅ REAL — the replay ring is implemented in the native C ABI (`mediaway_ffi`,
adr/container/0009); this example runs against it. Only the packets are synthetic.

LIMIT: the C ABI has no video-packet source. `EncodeSession` muxes its encoder's
packets internally and packet-level output exists only for the audio encoder, so a
real recorder feeds the ring packets from a `Demuxer`, from an `AudioEncoder`, or
from an encoder it drives itself. Here we make the packets up.

There is deliberately no one-shot "save this clip" call. A clip's packets go through
the same `Muxer` as any others: register each track once, then push the entries. The
muxer's track ids must match the ring's stream ids (the first `add_*_track` is 1).
"""

from datetime import timedelta

from mediaway import (
    AudioStreamInfo,
    Codec,
    Demuxer,
    Muxer,
    Packet,
    Rational,
    ReplayRing,
    VideoStreamInfo,
)

VIDEO_ID = 1  # what Muxer.add_video_track returns first
AUDIO_ID = 2
FPS = Rational(1, 30)
SECONDS = 6  # how much "recording" to make
WINDOW = timedelta(seconds=3)  # how much the ring keeps
CLIP = 2.0  # how much of it to save, in seconds


def video_packet(n: int) -> Packet:
    """Frame n at 30 fps; a keyframe every 30 frames. No zero bytes, so the muxer
    never mistakes the opaque payload for an Annex-B start code."""
    return Packet(
        stream_index=VIDEO_ID,
        pts=Rational(n, 30),
        payload=bytes([(n % 200) + 20]) * 256,
        key=n % 30 == 0,
        duration=Rational(1, 30),
    )


def audio_packet(k: int) -> Packet:
    """A fake AAC frame: 1024 samples at 48 kHz."""
    return Packet(
        stream_index=AUDIO_ID,
        pts=Rational(k * 1024, 48000),
        payload=bytes([(k % 200) + 20]) * 64,
        key=True,
        duration=Rational(1024, 48000),
    )


def main() -> None:
    frames = SECONDS * 30
    with ReplayRing(VIDEO_ID, FPS, WINDOW) as ring:
        ring.add_stream(AUDIO_ID, Rational(1, 48000))
        audio = 0
        for n in range(frames):
            ring.push(video_packet(n))
            # Audio arrives at ~46.9 packets/s; push what is due by this frame's time.
            while audio * 1024 / 48000 <= n / 30:
                ring.push(audio_packet(audio))
                audio += 1
        print(f"pushed {frames} video frames + {audio} audio packets; ring holds {ring.span()}")

        clip = ring.clip_last(CLIP)
        assert clip is not None, "no keyframe was pushed"
        # The ring can keep going while the clip is written, and can even be closed:
        # a clip is an owned snapshot.
        with clip:
            entries = list(clip)
            print(
                f"clip: {len(clip)} packets, {clip.duration} "
                f"(asked for {CLIP} s; a clip can start up to one keyframe interval earlier)"
            )

            # ── write the clip as a standalone MP4 ────────────────────────────
            with Muxer() as muxer:
                assert muxer.add_video_track(
                    VideoStreamInfo(codec=Codec.H264, width=640, height=480, frame_rate=FPS)
                ) == VIDEO_ID
                assert muxer.add_audio_track(
                    AudioStreamInfo(codec=Codec.AAC, sample_rate=48000, channels=2)
                ) == AUDIO_ID
                with muxer.begin() as live:
                    for entry in entries:
                        live.push_packet(entry.to_packet())
                    live.flush()
                    chunks = []
                    while (chunk := live.poll_bytes()) is not None:
                        chunks.append(chunk)
            mp4 = b"".join(chunks)
            print(f"muxed {len(mp4)} bytes")

    # ── prove it: demux the file and compare with the clip ───────────────────
    with Demuxer() as demuxer:
        demuxer.push_bytes(mp4)
        assert demuxer.stream_count() == 2
        demuxed = []
        while (packet := demuxer.poll_packet()) is not None:
            demuxed.append(packet)

    assert len(demuxed) == len(entries), f"{len(demuxed)} packets came back for a {len(entries)}-packet clip"
    first_video = next(p for p in demuxed if p.stream_index == VIDEO_ID)
    assert first_video.key, "the clip starts at a keyframe"
    assert first_video.pts == Rational(0, 1), f"the cut keyframe presents at zero, not {first_video.pts}"
    want = [e.payload for e in entries if e.stream_index == VIDEO_ID]
    got = [p.payload for p in demuxed if p.stream_index == VIDEO_ID]
    assert got == want, "the clip's video payloads survive the round trip"
    print(f"OK: the clip is a standalone MP4 of {len(demuxed)} packets starting at a keyframe")


if __name__ == "__main__":
    main()
