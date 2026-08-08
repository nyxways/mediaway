"""Auto video decode (encode -> mux -> demux -> decode) and Opus audio decode
(encode -> decode), round-tripped through the real native C ABI.

✅ REAL — the decode session C ABI (adr/0004-auto-decode-c-abi.md,
adr/pipeline/0006-audio-decode-c-abi.md) is implemented; this example runs
against it. Mirrors tests/test_decode_roundtrip.py's scenario, narrated for a
human reader instead of asserting.

DecodeSession/AudioDecodeSession are single-step handles (the handle IS the
decoder, same shape as AutoVideoEncoder/AudioEncoder) — NO_BACKEND raises
DecoderUnavailableError, an expected outcome to catch and exit gracefully.
"""

import math
import struct

from mediaway import (
    AudioDecodeSession,
    AudioEncoder,
    AutoVideoEncoder,
    Codec,
    DecodePacket,
    DecodeSession,
    DecoderUnavailableError,
    Demuxer,
    EncodeSession,
    EncoderUnavailableError,
    PixelFormat,
    Rational,
    VideoFrame,
)

WIDTH = 64
HEIGHT = 64
FRAME_COUNT = 10

SAMPLE_RATE = 48_000
CHANNELS = 1
FRAME_SAMPLES = 960  # 20ms @ 48kHz mono
AUDIO_FRAME_COUNT = 50


def video_roundtrip() -> None:
    try:
        encoder = AutoVideoEncoder.pick(codec=Codec.H264, width=WIDTH, height=HEIGHT, frame_rate=Rational(1, 30))
    except EncoderUnavailableError as err:
        print(f"skip: no H.264 encoder available: {err}")
        return

    nv12_len = WIDTH * HEIGHT + WIDTH * HEIGHT // 2
    plane = bytes([0x80]) * nv12_len
    with EncodeSession(encoder) as session:
        for i in range(FRAME_COUNT):
            session.push_frame(
                VideoFrame(width=WIDTH, height=HEIGHT, format=PixelFormat.NV12, data=plane, pts=Rational(i, 30))
            )
        fmp4 = session.finish()
    print(f"encoded {FRAME_COUNT} frames -> {len(fmp4)} fMP4 bytes")

    with Demuxer() as demuxer:
        demuxer.push_bytes(fmp4)
        video_stream = demuxer.streams()[0]
        packets = []
        while (packet := demuxer.poll_packet()) is not None:
            packets.append(packet)
    print(f"demuxed {len(packets)} H.264 packets, {len(video_stream.extra_data)} bytes of AVCC extra_data")

    try:
        decode_session = DecodeSession.open(
            codec=Codec.H264,
            width=WIDTH,
            height=HEIGHT,
            time_base=Rational(1, 30),
            extra_data=video_stream.extra_data,
        )
    except DecoderUnavailableError as err:
        print(f"skip: no H.264 decoder available: {err}")
        return

    with decode_session:
        for packet in packets:
            decode_session.push_packet(
                DecodePacket(pts=packet.pts, dts=packet.dts, duration=packet.duration, key=packet.key,
                             payload=packet.payload)
            )
        decode_session.flush()
        decoded = 0
        while decode_session.poll_frame() is not None:
            decoded += 1
    print(f"decoded {decoded} frames back")


def audio_roundtrip() -> None:
    tb = Rational(1, 50)
    try:
        encoder = AudioEncoder.open(codec=Codec.OPUS, sample_rate=SAMPLE_RATE, channels=CHANNELS, time_base=tb)
    except EncoderUnavailableError as err:
        print(f"skip: no Opus encoder available: {err}")
        return

    encoded = []
    with encoder:
        for i in range(AUDIO_FRAME_COUNT):
            samples = bytearray()
            for s in range(FRAME_SAMPLES):
                t = (i * FRAME_SAMPLES + s) / SAMPLE_RATE
                samples += struct.pack("<f", math.sin(t * 440.0 * 2 * math.pi))
            encoder.push_pcm(bytes(samples), pts=Rational(i, 50))
        encoder.flush()
        while (packet := encoder.poll_packet()) is not None:
            encoded.append(packet)
    print(f"encoded {len(encoded)} Opus packets")

    with AudioDecodeSession.open(sample_rate=SAMPLE_RATE, channels=CHANNELS, time_base=tb) as decode_session:
        for packet in encoded:
            decode_session.push_packet(DecodePacket(pts=packet.pts, payload=packet.payload))
        decode_session.flush()
        decoded = 0
        while decode_session.poll_frame() is not None:
            decoded += 1
    print(f"decoded {decoded} Opus frames back")


def main() -> None:
    video_roundtrip()
    audio_roundtrip()


if __name__ == "__main__":
    main()
