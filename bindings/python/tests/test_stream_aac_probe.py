"""Binding check for adr/pipeline/0007: streaming fMP4 bytes, AAC decode and the
capability probes, against the real native library.

Mirrors crates/mediaway-ffi/tests/{stream_bytes_smoke,aac_decode_smoke,
capability_probe_smoke}.rs, packaged as an assert-based script with no pytest
dependency (same style as test_decode_roundtrip.py).

Run from bindings/python:

    python tests/test_stream_aac_probe.py

The AAC encode/decode and streaming cases need this machine's real Windows
backends (WMF AAC + H.264); on other platforms they assert the documented
"unavailable" outcome instead. The Apple AAC arm is compile-checked in the Rust
workspace but has never been run, and is not exercised here.
"""

import ctypes
import math
import os
import platform
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from mediaway import (
    AudioDecodeSession,
    AudioEncoder,
    AutoVideoEncoder,
    Codec,
    DecoderUnavailableError,
    DecodePacket,
    Demuxer,
    EncodeBackend,
    EncodePathClass,
    EncodeSession,
    MediawayError,
    PixelFormat,
    Rational,
    SupportState,
    VideoFrame,
    decoder_support,
    encoder_support,
)
from mediaway import _ffi

IS_WINDOWS = platform.system() == "Windows"

WIDTH = 64
HEIGHT = 64
VIDEO_FRAMES = 100  # more than three default fragment batches (30 samples)

SAMPLE_RATE = 48_000
CHANNELS = 2
FRAME_SAMPLES = 1024
AUDIO_FRAMES = 48


def test_layout_matches_the_header() -> None:
    """Offsets derived by compiling a probe with gcc against the real pipeline.h
    (sizeof/offsetof), pinned here so a reordered field fails loudly."""
    cfg = _ffi.AudioDecodeConfig
    assert ctypes.sizeof(cfg) == 48
    assert [cfg.codec.offset, cfg.sample_rate.offset, cfg.channels.offset] == [0, 4, 8]
    assert cfg.time_base.offset == 16
    assert cfg.extra_data.offset == 32
    assert cfg.extra_data_len.offset == 40
    cap = _ffi.EncoderCapability
    assert ctypes.sizeof(cap) == 12
    assert [cap.backend.offset, cap.state.offset, cap.path_class.offset] == [0, 4, 8]
    loaded = _ffi.pipeline.dll.mediaway_pipeline_ffi_abi_version()
    assert loaded == _ffi.PIPELINE_ABI_VERSION == 7, f"loaded native lib is ABI {loaded}, binding expects 7"
    print("ok  layout + ABI version 7")


def _open_video_session() -> EncodeSession:
    encoder = AutoVideoEncoder.pick(codec=Codec.H264, width=WIDTH, height=HEIGHT, frame_rate=Rational(1, 30))
    return EncodeSession(encoder)


def _frame(i: int) -> VideoFrame:
    plane = bytes([0x80]) * (WIDTH * HEIGHT + WIDTH * HEIGHT // 2)
    return VideoFrame(width=WIDTH, height=HEIGHT, format=PixelFormat.NV12, data=plane, pts=Rational(i, 30))


def _count_packets(fmp4: bytes) -> int:
    with Demuxer() as demuxer:
        demuxer.push_bytes(fmp4)
        n = 0
        while demuxer.poll_packet() is not None:
            n += 1
        return n


def test_polled_bytes_plus_finish_tail_are_the_whole_stream() -> None:
    polled = bytearray()
    chunks = 0
    with _open_video_session() as session:
        assert session.poll_bytes() == b"", "nothing is ready before the first frame"
        for i in range(VIDEO_FRAMES):
            session.push_frame(_frame(i))
            chunk = session.poll_bytes()
            if chunk:
                chunks += 1
                polled += chunk
        tail = session.finish()
    assert chunks >= 1, f"no fMP4 bytes were ready before finish over {VIDEO_FRAMES} frames"
    assert bytes(polled[4:8]) == b"ftyp", "the first polled bytes start the file"
    assert 0 < len(tail) < len(polled), (
        f"finish after polling must return the unpolled tail ({len(tail)} B), "
        f"not the whole stream ({len(polled)} B polled)"
    )
    streamed = bytes(polled) + tail

    with _open_video_session() as session:
        for i in range(VIDEO_FRAMES):
            session.push_frame(_frame(i))
        reference = session.finish()

    assert _count_packets(streamed) == VIDEO_FRAMES, "streamed output must demux to every frame"
    assert _count_packets(reference) == VIDEO_FRAMES, "reference output must demux to every frame"
    print(f"ok  streaming: {chunks} non-empty polls, streamed {len(streamed)} B "
          f"(tail {len(tail)} B) vs unpolled {len(reference)} B")


def _sine_frame(i: int) -> bytes:
    out = bytearray()
    for s in range(FRAME_SAMPLES):
        t = (i * FRAME_SAMPLES + s) / SAMPLE_RATE
        v = math.sin(t * 440.0 * 2 * math.pi)
        out += struct.pack("<f", v) * CHANNELS
    return bytes(out)


def _encode_aac():
    with AudioEncoder.open(codec=Codec.AAC, sample_rate=SAMPLE_RATE, channels=CHANNELS) as encoder:
        for i in range(AUDIO_FRAMES):
            encoder.push_pcm(_sine_frame(i))
        encoder.flush()
        asc = encoder.stream_info().extra_data  # materialises after the first pushed frame
        packets = []
        while True:
            packet = encoder.poll_packet()
            if packet is None:
                break
            packets.append(packet)
    assert len(asc) > 0, "AAC AudioSpecificConfig expected"
    assert packets, "expected AAC packets"
    return asc, packets


def test_aac_encode_decode_round_trips() -> None:
    asc, packets = _encode_aac()
    tb = Rational(1, SAMPLE_RATE)
    samples = 0
    energy = 0.0
    with AudioDecodeSession.open(
        codec=Codec.AAC, sample_rate=SAMPLE_RATE, channels=CHANNELS, time_base=tb, extra_data=asc
    ) as session:
        for packet in packets:
            session.push_packet(DecodePacket(pts=packet.pts, duration=packet.duration, payload=packet.payload, key=True))
        session.flush()
        while True:
            frame = session.poll_frame()
            if frame is None:
                break
            assert (frame.sample_rate, frame.channels) == (SAMPLE_RATE, CHANNELS)
            values = struct.unpack(f"<{len(frame.data) // 4}f", frame.data)
            energy += sum(v * v for v in values)
            samples += len(values) // CHANNELS
    pushed = len(packets) * FRAME_SAMPLES
    assert samples == pushed, f"decoded {samples} samples per channel for {pushed} pushed"
    mean_square = energy / (samples * CHANNELS)
    assert mean_square > 0.1, f"decoded audio is near-silent (mean square {mean_square:.3f})"
    print(f"ok  AAC: {len(packets)} packets -> {samples} samples/ch, mean square {mean_square:.3f}")


def test_aac_open_and_push_rules() -> None:
    tb = Rational(1, SAMPLE_RATE)
    # An empty AudioSpecificConfig is a config mistake, not a missing capability.
    try:
        AudioDecodeSession.open(codec=Codec.AAC, sample_rate=SAMPLE_RATE, channels=CHANNELS, time_base=tb)
    except MediawayError as e:
        assert e.status == _ffi.PIPELINE_INVALID_INPUT, f"empty ASC must be invalid input, got status {e.status}"
    else:
        raise AssertionError("AAC open with an empty AudioSpecificConfig must fail")

    asc = bytes([0x11, 0x90])  # AAC-LC, 48 kHz, stereo
    with AudioDecodeSession.open(
        codec=Codec.AAC, sample_rate=SAMPLE_RATE, channels=CHANNELS, time_base=tb, extra_data=asc
    ) as session:
        try:
            session.push_packet(DecodePacket(pts=Rational(0, 1), payload=b""))
        except MediawayError as e:
            assert e.status == _ffi.PIPELINE_INVALID_INPUT, f"empty AAC packet must be invalid input, got {e.status}"
        else:
            raise AssertionError("an empty AAC packet must be refused (Opus's PLC hint means nothing for AAC)")
    print("ok  AAC open/push rules")


def test_probe_invariants_and_agreement() -> None:
    # Argument rules: zero dimensions are refused.
    try:
        encoder_support(Codec.H264, 0, 64)
    except MediawayError as e:
        assert e.status == _ffi.PIPELINE_INVALID_INPUT
    else:
        raise AssertionError("zero width must be invalid input")

    for codec in (Codec.H264, Codec.HEVC):
        for row in encoder_support(codec, 1280, 720):
            if row.state == SupportState.SUPPORTED:
                assert row.path_class != EncodePathClass.NONE, f"supported row without a path class: {row}"
            else:
                assert row.path_class == EncodePathClass.NONE, f"path_class only means something when SUPPORTED: {row}"
            assert isinstance(row.backend, EncodeBackend)

    assert decoder_support(Codec.H264) != SupportState.UNKNOWN
    if IS_WINDOWS:
        rows = encoder_support(Codec.H264, 1280, 720)
        assert rows, "Windows reports at least one backend row for H.264"
        any_supported = any(r.state == SupportState.SUPPORTED for r in rows)
        opened = True
        try:
            AutoVideoEncoder.pick(codec=Codec.H264, width=1280, height=720, frame_rate=Rational(1, 30)).close()
        except MediawayError:
            opened = False
        assert any_supported == opened, f"probe said supported={any_supported} but opening returned {opened}"

        # AAC decode: the probe must say so before a session is opened.
        assert decoder_support(Codec.AAC) == SupportState.SUPPORTED
        with AudioDecodeSession.open(
            codec=Codec.AAC, sample_rate=SAMPLE_RATE, channels=CHANNELS, time_base=Rational(1, SAMPLE_RATE),
            extra_data=bytes([0x11, 0x90]),
        ):
            pass
    print(f"ok  probes ({'Windows real backends' if IS_WINDOWS else 'invariants only'})")


def test_aac_unavailable_off_windows_and_apple() -> None:
    """Where there is no OS AAC decoder, open is DecoderUnavailableError (catch-and-continue)."""
    if IS_WINDOWS or platform.system() == "Darwin":
        return
    try:
        AudioDecodeSession.open(
            codec=Codec.AAC, sample_rate=SAMPLE_RATE, channels=CHANNELS, time_base=Rational(1, SAMPLE_RATE),
            extra_data=bytes([0x11, 0x90]),
        )
    except DecoderUnavailableError:
        pass
    else:
        raise AssertionError("AAC decode must be unavailable on this platform")
    print("ok  AAC unavailable off Windows/Apple")


if __name__ == "__main__":
    test_layout_matches_the_header()
    test_probe_invariants_and_agreement()
    test_aac_open_and_push_rules() if IS_WINDOWS else test_aac_unavailable_off_windows_and_apple()
    if IS_WINDOWS:
        test_polled_bytes_plus_finish_tail_are_the_whole_stream()
        test_aac_encode_decode_round_trips()
    print("PASS: streaming bytes + AAC decode + capability probes")
