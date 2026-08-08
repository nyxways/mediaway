"""mediaway — idiomatic Python binding over Mediaway's C ABI.

See bindings/python/README.md for the DX contract this package implements:
context managers, exceptions, Rational-second timestamps, bytes for buffers.

Capability truth (see the README's table): container mux/demux ✅ real,
auto video encode -> fMP4 ✅ real, camera/mic capture ✅ real (CPU frames),
Screen capture 🚧 unsupported by the ABI today (CaptureUnsupportedError).
"""

from ._container import Demuxer, LiveMuxer, Muxer
from ._container_adts import AdtsDemuxer, AdtsMuxer
from ._container_flv import AUDIO_TRACK_ID as FLV_AUDIO_TRACK_ID
from ._container_flv import VIDEO_TRACK_ID as FLV_VIDEO_TRACK_ID
from ._container_flv import FlvDemuxer, FlvMuxer
from ._container_mp3 import Mp3Demuxer, Mp3Muxer
from ._container_ogg import OggDemuxer, OggMuxer
from ._container_ts import TsDemuxer, TsMuxer
from ._container_wav import WavMuxer
from ._container_wav import parse as wav_parse
from ._device import AudioCapture, VideoCapture
from ._encoder import AudioEncoder, AutoVideoEncoder, EncodeSession
from ._errors import (
    CaptureUnsupportedError,
    DeviceUnavailableError,
    EncoderUnavailableError,
    InvalidStateError,
    MediawayError,
)
from ._ffi import lib_dir
from ._types import (
    AudioStreamInfo,
    ChannelMode,
    Codec,
    ContainerFormat,
    Mp3FrameHeader,
    MpegVersion,
    Packet,
    PixelFormat,
    Rational,
    RawPacket,
    SampleFormat,
    TsElementaryStream,
    VideoFrame,
    VideoStreamInfo,
    WavSampleFormat,
    WaveFormat,
)

__version__ = "0.1.0"

__all__ = [
    "Muxer",
    "LiveMuxer",
    "Demuxer",
    "ContainerFormat",
    "OggMuxer",
    "OggDemuxer",
    "AdtsMuxer",
    "AdtsDemuxer",
    "FlvMuxer",
    "FlvDemuxer",
    "FLV_VIDEO_TRACK_ID",
    "FLV_AUDIO_TRACK_ID",
    "TsMuxer",
    "TsDemuxer",
    "TsElementaryStream",
    "Mp3Muxer",
    "Mp3Demuxer",
    "Mp3FrameHeader",
    "MpegVersion",
    "ChannelMode",
    "WavMuxer",
    "wav_parse",
    "WaveFormat",
    "WavSampleFormat",
    "AutoVideoEncoder",
    "EncodeSession",
    "AudioEncoder",
    "VideoCapture",
    "AudioCapture",
    "MediawayError",
    "EncoderUnavailableError",
    "DeviceUnavailableError",
    "CaptureUnsupportedError",
    "InvalidStateError",
    "Rational",
    "Codec",
    "PixelFormat",
    "SampleFormat",
    "VideoStreamInfo",
    "AudioStreamInfo",
    "Packet",
    "RawPacket",
    "VideoFrame",
    "lib_dir",
]
