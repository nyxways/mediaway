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
    "GpuAdapter",
    "VideoStreamInfo",
    "AudioStreamInfo",
    "Packet",
    "RawPacket",
    "VideoFrame",
    "DecodePacket",
    "DecodedVideoFrame",
    "DecodedAudioFrame",
    "ContainerFormat",
    "MpegVersion",
    "ChannelMode",
    "WavSampleFormat",
    "Mp3FrameHeader",
    "WaveFormat",
    "TsElementaryStream",
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
    VP8 = 12


class ContainerFormat(enum.IntEnum):
    """Which format `Muxer`/`Demuxer` open — mirrors mediaway_container_format_t.

    Only formats sharing MP4's multi-track, typestated shape are reachable
    here; Ogg/ADTS/FLV/MPEG-TS/MP3/WAV have their own dedicated classes.
    """

    MP4 = 0
    WEBM = 1


class MpegVersion(enum.IntEnum):
    """MPEG audio version — mirrors mediaway_mpeg_version_t."""

    MPEG1 = 0  # 44100/48000/32000 Hz family
    MPEG2 = 1  # 22050/24000/16000 Hz family
    MPEG2_5 = 2  # 11025/12000/8000 Hz family (unofficial low-rate extension)


class ChannelMode(enum.IntEnum):
    """MPEG Layer III channel mode — mirrors mediaway_channel_mode_t."""

    STEREO = 0
    JOINT_STEREO = 1
    DUAL_CHANNEL = 2
    MONO = 3


class WavSampleFormat(enum.IntEnum):
    """RIFF/WAVE fmt chunk sample encoding (wFormatTag) — mirrors mediaway_wav_sample_format_t.

    NOT the same enum as `SampleFormat` (raw PCM bit depth for device/pipeline
    audio) — this is the WAVE container's own tag encoding.
    """

    PCM = 0
    FLOAT = 1


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
class GpuAdapter:
    """One enumerated DXGI adapter — mirrors mediaway_gpu_adapter_info_t.
    Returned by `GpuDevice.list_adapters()`."""

    index: int
    name: str
    vendor_id: int
    device_id: int
    dedicated_video_memory: int  # bytes
    is_hardware: bool


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


@dataclass(frozen=True)
class DecodePacket:
    """Input to `DecodeSession.push_packet`/`AudioDecodeSession.push_packet` —
    a pipeline-scoped packet view, distinct from `Packet` above
    (adr/pipeline/0006-audio-decode-c-abi.md §4: shared by both video and
    audio decode). `payload` is a plain bytes copy, borrowed for the call
    only on the native side.

    For audio: an empty `payload` is Opus's packet-loss-concealment hint for
    a lost frame, not an error — pass it whenever a frame is known lost.
    """

    pts: Rational
    payload: bytes
    dts: Rational | None = None
    key: bool = False
    duration: Rational | None = None


@dataclass(frozen=True)
class DecodedVideoFrame:
    """Output of `DecodeSession.poll_frame` — CPU-only (GPU decode output is
    deferred, adr/0004-auto-decode-c-abi.md §1/§5). `data` is a plain bytes
    copy."""

    width: int
    height: int
    format: PixelFormat
    data: bytes
    pts: Rational
    duration: Rational | None = None  # None = unknown


@dataclass(frozen=True)
class DecodedAudioFrame:
    """Output of `AudioDecodeSession.poll_frame` — always interleaved F32
    PCM (adr/pipeline/0006-audio-decode-c-abi.md § Decode side). `data` is a
    plain bytes copy."""

    sample_rate: int
    channels: int
    data: bytes
    pts: Rational
    duration: Rational | None = None  # None = unknown


@dataclass(frozen=True)
class RawPacket:
    """One packet using ABI-native raw integer pts/dts/duration, not Rational
    seconds — used by the dedicated Ogg/ADTS/FLV/MPEG-TS/MP3/WAV wrappers,
    none of which have MP4/WebM's per-track time_base to convert seconds
    against (e.g. Ogg's pts IS the granule position; MPEG-TS's is the raw
    90 kHz system clock)."""

    stream_id: int
    pts: int
    dts: int
    payload: bytes
    duration: int = 0
    key: bool = False
    discard: bool = False


@dataclass(frozen=True)
class Mp3FrameHeader:
    """Fixed Layer III frame header for `Mp3Muxer` — bitrate/sample rate/
    channel mode stay constant for the whole mux session's lifetime."""

    version: MpegVersion
    bitrate_kbps: int  # must be one of the 14 standard Layer III rates for `version`
    sample_rate: int  # must be one of the 3 standard rates for `version`
    channel_mode: ChannelMode


@dataclass(frozen=True)
class WaveFormat:
    """Explicit RIFF/WAVE fmt chunk for `WavMuxer`."""

    sample_format: WavSampleFormat
    channels: int
    sample_rate: int
    bits_per_sample: int


@dataclass(frozen=True)
class TsElementaryStream:
    """One elementary stream registered in `TsMuxer`'s constructed PMT."""

    pid: int  # must be in 2..=0x1FFF (0/1 are reserved for PAT/CAT)
    codec: Codec  # must be H264, HEVC, AAC, or MP3
