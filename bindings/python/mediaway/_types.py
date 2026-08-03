"""Public value types for the mediaway Python package.

Dataclasses in, plain values out: these types are what examples construct and
read; the wrappers convert them to/from the C ABI structs in `_ffi`.

Timestamps (`pts`/`dts`) are `Rational` seconds — e.g. frame i of a 30 fps
stream is `Rational(i, 30)`. The wrappers convert to/from the ABI's integer
time-base units on the way in/out.
"""

from __future__ import annotations

import enum
from dataclasses import dataclass, field

__all__ = [
    "Codec",
    "PixelFormat",
    "SampleFormat",
    "Rational",
    "VideoStreamInfo",
    "AudioStreamInfo",
    "Packet",
    "VideoFrame",
]


class Codec(enum.IntEnum):
    """Mirror of the ABI's mediaway_codec_kind values (1:1 with Rust CodecKind)."""

    H264 = 0
    HEVC = 1
    AV1 = 2
    VP9 = 3
    AAC = 4
    OPUS = 5
    MP3 = 6
    VORBIS = 7
    WEBVTT = 8
    TX3G = 9
    RAW_VIDEO = 10
    RAW_AUDIO = 11


class PixelFormat(enum.IntEnum):
    """Mirror of the ABI's mediaway_pixel_format values."""

    NV12 = 0
    I420 = 1
    BGRA8 = 2
    RGBA8 = 3
    YUYV = 4


class SampleFormat(enum.IntEnum):
    """Mirror of the ABI's mediaway_sample_format values."""

    S16 = 0
    S32 = 1
    F32 = 2


@dataclass(frozen=True)
class Rational:
    """num/den seconds. `den` must be non-zero."""

    num: int
    den: int

    def __str__(self) -> str:
        return f"{self.num}/{self.den}"


@dataclass(frozen=True)
class VideoStreamInfo:
    """A video track. `frame_rate` is the frame period: Rational(1, 30) = 30 fps."""

    codec: Codec
    width: int
    height: int
    frame_rate: Rational
    extra_data: bytes = b""  # codec config (e.g. avcC); empty when unknown


@dataclass(frozen=True)
class AudioStreamInfo:
    """An audio track."""

    codec: Codec
    sample_rate: int
    channels: int
    extra_data: bytes = b""  # codec config (e.g. esds); empty when unknown


@dataclass(frozen=True)
class Packet:
    """One muxed/demuxed packet. `pts`/`dts` are Rational seconds; `dts`
    defaults to `pts` when omitted. `payload` is a plain bytes copy."""

    stream_index: int
    pts: Rational
    payload: bytes
    dts: Rational | None = None
    key: bool = False  # sync sample / keyframe
    duration: Rational | None = None  # Rational seconds; None = unknown


@dataclass(frozen=True)
class VideoFrame:
    """One video frame, CPU-storage (see the README's capability truth table)."""

    width: int
    height: int
    format: PixelFormat
    data: bytes
    pts: Rational
    duration: Rational | None = None  # None = unknown
