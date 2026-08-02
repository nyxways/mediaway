"""Mux + demux roundtrip — aspirational quick-start example.

ASPIRATIONAL EXAMPLE: no `mediaway` Python package exists yet. This file shows
the target ergonomics for a future Python binding over Mediaway's C ABI
(ctypes/cffi under the hood, wrapped in idiomatic Python: context managers,
exceptions, snake_case). See ../README.md and docs/spec/c-ffi.md.

Mirrors examples/mux_roundtrip.rs: register one H.264 video track and one AAC
audio track, push fake packets for a simulated 3-second clip, flush, and read
the fragmented MP4 bytes back with a streaming demuxer.

Run (once the real package exists):
    python mux_roundtrip.py
"""

from __future__ import annotations

from dataclasses import dataclass

from mediaway import (
    AudioStreamInfo,
    Codec,
    Demuxer,
    Muxer,
    Packet,
    Rational,
    VideoStreamInfo,
)


@dataclass(frozen=True)
class ClipPlan:
    """Parameters for the synthetic clip we mux."""

    fps: Rational = Rational(1, 30)
    sample_rate_base: Rational = Rational(1, 48_000)
    frame_count: int = 90  # 3 s at 30 fps
    keyframe_interval: int = 30


def build_fmp4(plan: ClipPlan) -> bytes:
    """Mux one video + one audio track into fragmented MP4 bytes."""
    muxer = Muxer()

    # ── 1. Register tracks (open state) ──────────────────────────────────
    video_id = muxer.add_track(
        VideoStreamInfo(
            codec=Codec.H264,
            time_base=plan.fps,
            width=1920,
            height=1080,
            extra_data=b"",
        )
    )
    audio_id = muxer.add_track(
        AudioStreamInfo(
            codec=Codec.AAC,
            time_base=plan.sample_rate_base,
            extra_data=b"",
            sample_rate=48_000,
            channels=2,
        )
    )

    # ── 2. Transition to a live session — track registration closes here ──
    with muxer.begin() as session:
        for i in range(plan.frame_count):
            session.push_packet(
                Packet(
                    stream_id=video_id,
                    pts=i,
                    dts=i,
                    duration=1,
                    is_keyframe=(i % plan.keyframe_interval == 0),
                    is_discard=False,
                    payload=b"\x00\x00\x00\x01",  # placeholder NAL unit
                )
            )
            session.push_packet(
                Packet(
                    stream_id=audio_id,
                    pts=i * 1_600,
                    dts=i * 1_600,
                    duration=1_600,
                    is_keyframe=True,
                    is_discard=False,
                    payload=b"\xff\xf1",
                )
            )
        session.flush()

        # ── 3. Pull bytes — caller owns I/O, the muxer never touches disk ──
        return session.poll_bytes()


def demux_and_count(data: bytes) -> tuple[int, int]:
    """Feed muxed bytes into a demuxer and count video vs. audio packets."""
    with Demuxer() as demuxer:
        demuxer.push_bytes(data)

        streams = demuxer.streams()
        print(f"mux_roundtrip: demuxer sees {len(streams)} stream(s)")
        for stream in streams:
            if stream.geometry is not None:
                print(
                    f"  stream {stream.id} — {stream.codec.name} "
                    f"{stream.geometry.width}x{stream.geometry.height}"
                )
            else:
                print(f"  stream {stream.id} — {stream.codec.name} (no geometry)")

        n_video = 0
        n_audio = 0
        while (packet := demuxer.poll_packet()) is not None:
            stream = next(s for s in streams if s.id == packet.stream_id)
            if stream.codec == Codec.H264:
                n_video += 1
            else:
                n_audio += 1
        return n_video, n_audio


if __name__ == "__main__":
    plan = ClipPlan()
    fmp4_bytes = build_fmp4(plan)
    print(f"mux_roundtrip: {plan.frame_count} frames -> {len(fmp4_bytes)} bytes of fMP4")

    n_video, n_audio = demux_and_count(fmp4_bytes)
    print(f"mux_roundtrip: recovered {n_video} video + {n_audio} audio packets")
    assert n_video > 0
    print("mux_roundtrip: OK")
