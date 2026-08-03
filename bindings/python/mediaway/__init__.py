"""mediaway — idiomatic Python binding over Mediaway's C ABI.

See bindings/python/README.md for the DX contract this package implements:
context managers, exceptions, Rational-second timestamps, bytes for buffers.

Capability truth (see the README's table): container mux/demux ✅ real,
auto video encode -> fMP4 ✅ real, camera/mic capture ✅ real (CPU frames),
Screen capture 🚧 unsupported by the ABI today (CaptureUnsupportedError).
"""

from ._container import Demuxer, LiveMuxer, Muxer
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
    Codec,
    Packet,
    PixelFormat,
    Rational,
    SampleFormat,
    VideoFrame,
    VideoStreamInfo,
)

__version__ = "0.1.0"

__all__ = [
    "Muxer",
    "LiveMuxer",
    "Demuxer",
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
    "VideoFrame",
    "lib_dir",
]
