"""RC-stage binding check: video decode (encode->mux->demux->decode) and
Opus audio decode (encode->decode), round-tripped through the release-built
native library (mediaway_ffi.dll on Windows, libmediaway_ffi.so on Linux).

Mirrors crates/mediaway-ffi/tests/{decode,audio_decode}_smoke.rs and the
C++/C# bindings' own decode round-trip tests, packaged as an assert-based
script with no pytest dependency (same style as test_mux_roundtrip.py).

Run from bindings/python (the native library must be staged at
mediaway/_native/):

    python tests/test_decode_roundtrip.py

A failed assertion raises AssertionError and exits nonzero, which is the RC
job's failure signal. EncoderUnavailableError/DecoderUnavailableError are
treated as real failures for the video case (this machine has a
hardware-verified WMF H.264 backend); the Opus path is cross-platform.
"""

import math
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from mediaway import (
    AudioDecodeSession,
    AudioEncoder,
    AutoVideoEncoder,
    Codec,
    DecodePacket,
    DecodeSession,
    Demuxer,
    EncodeSession,
    PixelFormat,
    Rational,
    VideoFrame,
    VideoStreamInfo,
)

WIDTH = 64
HEIGHT = 64
FRAME_COUNT = 10

SAMPLE_RATE = 48_000
CHANNELS = 1
FRAME_SAMPLES = 960  # 20ms @ 48kHz mono
AUDIO_FRAME_COUNT = 50


def run_video_roundtrip() -> None:
    # ── encode + mux (mid-gray NV12, no real capture needed) ──────────────
    encoder = AutoVideoEncoder.pick(codec=Codec.H264, width=WIDTH, height=HEIGHT, frame_rate=Rational(1, 30))
    nv12_len = WIDTH * HEIGHT + WIDTH * HEIGHT // 2
    plane = bytes([0x80]) * nv12_len
    with EncodeSession(encoder) as session:
        for i in range(FRAME_COUNT):
            session.push_frame(
                VideoFrame(width=WIDTH, height=HEIGHT, format=PixelFormat.NV12, data=plane, pts=Rational(i, 30))
            )
        fmp4 = session.finish()
    assert len(fmp4) > 0, "encoder produced no bytes"

    # ── demux: recover real H.264 packets + AVCC extra_data ────────────────
    with Demuxer() as demuxer:
        demuxer.push_bytes(fmp4)
        streams = demuxer.streams()
        assert len(streams) == 1, f"expected 1 stream, got {len(streams)}"
        video_stream = streams[0]
        assert isinstance(video_stream, VideoStreamInfo)
        assert len(video_stream.extra_data) > 0, "expected AVCC extra_data"

        packets = []
        while True:
            packet = demuxer.poll_packet()
            if packet is None:
                break
            packets.append(packet)
    assert len(packets) == FRAME_COUNT, "every frame must demux back"

    # ── decode: extra_data supplied at open time (adr/0004 §1) ────────────
    with DecodeSession.open(
        codec=Codec.H264, width=WIDTH, height=HEIGHT, time_base=Rational(1, 30), extra_data=video_stream.extra_data
    ) as decode_session:
        for packet in packets:
            decode_session.push_packet(
                DecodePacket(pts=packet.pts, dts=packet.dts, duration=packet.duration, key=packet.key,
                             payload=packet.payload)
            )
        decode_session.flush()

        decoded = 0
        while True:
            frame = decode_session.poll_frame()
            if frame is None:
                break
            assert (frame.width, frame.height) == (WIDTH, HEIGHT), "decoded frame geometry mismatch"
            assert len(frame.data) >= nv12_len, "decoded frame implausibly small"
            decoded += 1
    assert decoded > 0, "expected at least one decoded frame"
    print(f"video: encoded {len(fmp4)} bytes, decoded {decoded} frames")


def run_audio_roundtrip() -> None:
    # ── encode: Python's AudioEncoder already accepts codec=Codec.OPUS ────
    tb = Rational(1, 50)
    with AudioEncoder.open(codec=Codec.OPUS, sample_rate=SAMPLE_RATE, channels=CHANNELS, time_base=tb) as encoder:
        encoded = []
        for i in range(AUDIO_FRAME_COUNT):
            samples = bytearray()
            for s in range(FRAME_SAMPLES):
                t = (i * FRAME_SAMPLES + s) / SAMPLE_RATE
                v = math.sin(t * 440.0 * 2 * math.pi)
                samples += struct.pack("<f", v)
            encoder.push_pcm(bytes(samples), pts=Rational(i, 50))
        encoder.flush()
        while True:
            packet = encoder.poll_packet()
            if packet is None:
                break
            encoded.append(packet)
    assert len(encoded) > 0, "expected at least one encoded Opus packet"

    # ── decode via the public wrapper ───────────────────────────────────────
    with AudioDecodeSession.open(sample_rate=SAMPLE_RATE, channels=CHANNELS, time_base=tb) as decode_session:
        for packet in encoded:
            decode_session.push_packet(DecodePacket(pts=packet.pts, payload=packet.payload))
        decode_session.flush()

        decoded = 0
        while True:
            frame = decode_session.poll_frame()
            if frame is None:
                break
            assert (frame.sample_rate, frame.channels) == (SAMPLE_RATE, CHANNELS), "decoded audio format mismatch"
            decoded += 1
    assert decoded > 0, "expected at least one decoded Opus frame"
    print(f"audio: encoded {len(encoded)} Opus packets, decoded {decoded} frames")


if __name__ == "__main__":
    run_video_roundtrip()
    run_audio_roundtrip()
    print("PASS: video decode + Opus audio decode round-tripped through the native library")
