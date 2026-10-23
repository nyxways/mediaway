"""Shared ctypes layer: library discovery, value types and the container C ABI.

Part of the raw ABI layer that `mediaway/_ffi.py` re-exports as one namespace: struct layouts and function prototypes,
mirroring `crates/mediaway-*-ffi/include/mediaway/*.h` exactly. Nothing here
is idiomatic — the wrappers in `mediaway/_container.py`, `_encoder.py`, and
`_device.py` translate this into Python.

Ownership rules (from the headers):
  - Borrowed inputs (track extra_data, packet payload, push_bytes data, frame
    raw_bytes, decryption key) are caller-owned, valid for the call only. The
    wrappers copy in/out so Python callers never hold native memory.
  - Owned outputs (poll_bytes buffers, demuxed packets/stream info, finish
    buffers, polled device frames) MUST be released through the matching
    `_free` function — the wrappers do this automatically.
"""

from __future__ import annotations

import ctypes as _c
import os
import platform
from ctypes import (
    POINTER,
    Structure,
    byref,
    c_bool,
    c_char_p,
    c_int32,
    c_int64,
    c_size_t,
    c_uint16,
    c_uint32,
    c_uint64,
    c_void_p,
)

# ── Library discovery ────────────────────────────────────────────────────────
#
# The cdylibs are Rust build artifacts, not installed system libraries. We look
# in, in order:
#   1. $MEDIAWAY_FFI_DIR
#   2. <package>/_native/                          (DLLs bundled in the wheel — the PyPI distribution)
#   3. <repo root>/target/x86_64-pc-windows-gnu/debug   (GNU toolchain, C examples — Windows only)
#   4. <repo root>/target/debug                          (host toolchain — MSVC on Windows, native on Linux)
#   5. the current working directory
# <repo root> is derived from this file's location (bindings/python/mediaway/).

_THIS_DIR = os.path.dirname(os.path.abspath(__file__))
_REPO_ROOT = os.path.abspath(os.path.join(_THIS_DIR, "..", "..", ".."))

_SEARCH_DIRS = [
    os.environ.get("MEDIAWAY_FFI_DIR", ""),
    os.path.join(_THIS_DIR, "_native"),
    os.path.join(_REPO_ROOT, "target", "x86_64-pc-windows-gnu", "debug"),
    os.path.join(_REPO_ROOT, "target", "debug"),
    os.getcwd(),
]


def _library_filename() -> str:
    """cdylib filename Cargo produces for this platform."""
    system = platform.system()
    if system == "Windows":
        return "mediaway_ffi.dll"
    if system == "Linux":
        return "libmediaway_ffi.so"
    if system == "Darwin":
        return "libmediaway_ffi.dylib"
    raise OSError(f"mediaway: unsupported platform {system!r}")


_LIBRARY_FILENAME = _library_filename()


def _load_library(name: str) -> _c.CDLL:
    for d in _SEARCH_DIRS:
        if not d:
            continue
        candidate = os.path.join(d, name)
        if os.path.isfile(candidate):
            return _c.CDLL(str(candidate))
    raise OSError(
        f"cannot find {name}; set $MEDIAWAY_FFI_DIR or build the -ffi crates "
        f"(searched: {[d for d in _SEARCH_DIRS if d]})"
    )


def lib_dir() -> str:
    """Directory containing the loaded Mediaway DLLs (for PATH staging)."""
    return os.path.dirname(container.dll._name)


class _Library:
    """Lazy loader for one Mediaway -ffi cdylib."""

    def __init__(self, filename: str):
        self._filename = filename
        self._dll: _c.CDLL | None = None

    @property
    def dll(self) -> _c.CDLL:
        if self._dll is None:
            self._dll = _load_library(self._filename)
        return self._dll


container = _Library(_LIBRARY_FILENAME)
pipeline = _Library(_LIBRARY_FILENAME)
device = _Library(_LIBRARY_FILENAME)


# ── Shared value types (identical layout across the three headers) ──────────

class Rational(Structure):
    _fields_ = [
        ("num", c_uint64),
        ("den", c_uint32),  # must be non-zero
    ]


# ── container.h: status codes ────────────────────────────────────────────────

# enum mediaway_status
MEDIAWAY_OK = 0
MEDIAWAY_STATUS_INVALID_ARGUMENT = 1
MEDIAWAY_STATUS_INVALID_STATE = 2
MEDIAWAY_STATUS_INVALID_TRACK = 3
MEDIAWAY_STATUS_INVALID_PACKET = 4
MEDIAWAY_STATUS_INVALID_DATA = 5
MEDIAWAY_STATUS_UNKNOWN_ERROR = 6
MEDIAWAY_STATUS_INTERNAL_PANIC = 7
MEDIAWAY_STATUS_HANDLE_POISONED = 8
MEDIAWAY_STATUS_UNSUPPORTED_CODEC = 9  # track's codec has no encoding in the requested format
MEDIAWAY_STATUS_UNKNOWN_STREAM = 10  # push_packet's stream_id matches no registered track

# enum mediaway_codec_kind
CODEC_H264 = 0
CODEC_HEVC = 1
CODEC_AV1 = 2
CODEC_VP9 = 3
CODEC_AAC = 4
CODEC_OPUS = 5
CODEC_MP3 = 6
CODEC_VORBIS = 7
CODEC_WEBVTT = 8
CODEC_TX3G = 9
CODEC_RAW_VIDEO = 10
CODEC_RAW_AUDIO = 11
CODEC_VP8 = 12

# enum mediaway_container_format — only MP4/WebM share the generic
# mediaway_muxer_t/mediaway_demuxer_t shape; Ogg/ADTS/FLV/MPEG-TS/MP3/WAV each
# get their own dedicated handles below.
CONTAINER_FORMAT_MP4 = 0
CONTAINER_FORMAT_WEBM = 1

c_ubyte = _c.c_ubyte
U8P = POINTER(c_ubyte)


# Borrowed input to add_video_track
class VideoTrackInfo(Structure):
    _fields_ = [
        ("id", c_uint32),
        ("codec", c_int32),
        ("time_base", Rational),
        ("width", c_uint32),
        ("height", c_uint32),
        ("extra_data", U8P),  # borrowed
        ("extra_data_len", c_size_t),
    ]


class AudioTrackInfo(Structure):
    _fields_ = [
        ("id", c_uint32),
        ("codec", c_int32),
        ("time_base", Rational),
        ("sample_rate", c_uint32),
        ("channels", c_uint16),
        ("extra_data", U8P),  # borrowed
        ("extra_data_len", c_size_t),
    ]


class PacketView(Structure):
    _fields_ = [
        ("stream_id", c_uint32),
        ("pts", c_int64),
        ("dts", c_int64),
        ("duration", c_uint64),
        ("is_keyframe", c_bool),
        ("is_discard", c_bool),
        ("payload", U8P),  # borrowed
        ("payload_len", c_size_t),
    ]


class Packet(Structure):  # owned output
    _fields_ = [
        ("stream_id", c_uint32),
        ("pts", c_int64),
        ("dts", c_int64),
        ("duration", c_uint64),
        ("is_keyframe", c_bool),
        ("is_discard", c_bool),
        ("payload", U8P),  # owned
        ("payload_len", c_size_t),
    ]


class StreamInfo(Structure):  # owned output
    _fields_ = [
        ("id", c_uint32),
        ("codec", c_int32),
        ("time_base", Rational),
        ("has_geometry", c_bool),
        ("width", c_uint32),
        ("height", c_uint32),
        ("sample_rate", c_uint32),
        ("channels", c_uint16),
        ("extra_data", U8P),  # owned
        ("extra_data_len", c_size_t),
    ]


# ── container.h: functions ────────────────────────────────────────────────────

_H = container.dll

_H.mediaway_container_ffi_abi_version.restype = c_uint32
_H.mediaway_container_ffi_abi_version.argtypes = []

_H.mediaway_muxer_create.restype = c_void_p
_H.mediaway_muxer_create.argtypes = []
_H.mediaway_muxer_create_for_format.restype = c_void_p
_H.mediaway_muxer_create_for_format.argtypes = [c_int32]
_H.mediaway_muxer_create_with_fragment_batch.restype = c_void_p
_H.mediaway_muxer_create_with_fragment_batch.argtypes = [c_size_t]
_H.mediaway_muxer_add_video_track.restype = c_int32
_H.mediaway_muxer_add_video_track.argtypes = [c_void_p, POINTER(VideoTrackInfo)]
_H.mediaway_muxer_add_audio_track.restype = c_int32
_H.mediaway_muxer_add_audio_track.argtypes = [c_void_p, POINTER(AudioTrackInfo)]
_H.mediaway_muxer_begin.restype = c_int32
_H.mediaway_muxer_begin.argtypes = [c_void_p]
_H.mediaway_muxer_push_packet.restype = c_int32
_H.mediaway_muxer_push_packet.argtypes = [c_void_p, POINTER(PacketView)]
_H.mediaway_muxer_flush.restype = c_int32
_H.mediaway_muxer_flush.argtypes = [c_void_p]
_H.mediaway_muxer_poll_bytes.restype = c_int32
_H.mediaway_muxer_poll_bytes.argtypes = [c_void_p, POINTER(U8P), POINTER(c_size_t)]
_H.mediaway_muxer_close.restype = None
_H.mediaway_muxer_close.argtypes = [c_void_p]

_H.mediaway_demuxer_create.restype = c_void_p
_H.mediaway_demuxer_create.argtypes = []
_H.mediaway_demuxer_create_for_format.restype = c_void_p
_H.mediaway_demuxer_create_for_format.argtypes = [c_int32]
_H.mediaway_demuxer_push_bytes.restype = c_int32
_H.mediaway_demuxer_push_bytes.argtypes = [c_void_p, U8P, c_size_t]
_H.mediaway_demuxer_stream_count.restype = c_size_t
_H.mediaway_demuxer_stream_count.argtypes = [c_void_p]
_H.mediaway_demuxer_stream_at.restype = c_int32
_H.mediaway_demuxer_stream_at.argtypes = [c_void_p, c_size_t, POINTER(StreamInfo)]
_H.mediaway_demuxer_poll_packet.restype = c_int32
_H.mediaway_demuxer_poll_packet.argtypes = [c_void_p, POINTER(Packet), POINTER(c_bool)]
_H.mediaway_demuxer_set_decryption_key.restype = c_int32
_H.mediaway_demuxer_set_decryption_key.argtypes = [c_void_p, U8P, c_size_t]
_H.mediaway_demuxer_clear_decryption_key.restype = c_int32
_H.mediaway_demuxer_clear_decryption_key.argtypes = [c_void_p]
_H.mediaway_demuxer_close.restype = None
_H.mediaway_demuxer_close.argtypes = [c_void_p]

_H.mediaway_buffer_free.restype = None
_H.mediaway_buffer_free.argtypes = [U8P, c_size_t]
_H.mediaway_packet_free.restype = None
_H.mediaway_packet_free.argtypes = [POINTER(Packet)]
_H.mediaway_stream_info_free.restype = None
_H.mediaway_stream_info_free.argtypes = [POINTER(StreamInfo)]


# ── container.h: Ogg (adr/container/0004) ────────────────────────────────────
# Dedicated handles, not mediaway_muxer_t/mediaway_demuxer_t: Ogg has no
# track-registration step and no Open/Live typestate. Reuses PacketView/
# Packet/StreamInfo and the shared frees above.

_H = container.dll

_H.mediaway_ogg_muxer_create.restype = c_void_p
_H.mediaway_ogg_muxer_create.argtypes = [c_uint32]
_H.mediaway_ogg_muxer_push_packet.restype = c_int32
_H.mediaway_ogg_muxer_push_packet.argtypes = [c_void_p, POINTER(PacketView)]
_H.mediaway_ogg_muxer_flush.restype = c_int32
_H.mediaway_ogg_muxer_flush.argtypes = [c_void_p]
_H.mediaway_ogg_muxer_poll_bytes.restype = c_int32
_H.mediaway_ogg_muxer_poll_bytes.argtypes = [c_void_p, POINTER(U8P), POINTER(c_size_t)]
_H.mediaway_ogg_muxer_close.restype = None
_H.mediaway_ogg_muxer_close.argtypes = [c_void_p]

_H.mediaway_ogg_demuxer_create.restype = c_void_p
_H.mediaway_ogg_demuxer_create.argtypes = []
_H.mediaway_ogg_demuxer_push_bytes.restype = c_int32
_H.mediaway_ogg_demuxer_push_bytes.argtypes = [c_void_p, U8P, c_size_t]
_H.mediaway_ogg_demuxer_stream_count.restype = c_size_t
_H.mediaway_ogg_demuxer_stream_count.argtypes = [c_void_p]
_H.mediaway_ogg_demuxer_stream_at.restype = c_int32
_H.mediaway_ogg_demuxer_stream_at.argtypes = [c_void_p, c_size_t, POINTER(StreamInfo)]
_H.mediaway_ogg_demuxer_poll_packet.restype = c_int32
_H.mediaway_ogg_demuxer_poll_packet.argtypes = [c_void_p, POINTER(Packet), POINTER(c_bool)]
_H.mediaway_ogg_demuxer_close.restype = None
_H.mediaway_ogg_demuxer_close.argtypes = [c_void_p]

# ── container.h: ADTS (adr/container/0004) ───────────────────────────────────
# Same dedicated-handle reasoning as Ogg above.

_H.mediaway_adts_muxer_create.restype = c_void_p
_H.mediaway_adts_muxer_create.argtypes = [c_uint32, c_ubyte]
_H.mediaway_adts_muxer_push_packet.restype = c_int32
_H.mediaway_adts_muxer_push_packet.argtypes = [c_void_p, POINTER(PacketView)]
_H.mediaway_adts_muxer_flush.restype = c_int32
_H.mediaway_adts_muxer_flush.argtypes = [c_void_p]
_H.mediaway_adts_muxer_poll_bytes.restype = c_int32
_H.mediaway_adts_muxer_poll_bytes.argtypes = [c_void_p, POINTER(U8P), POINTER(c_size_t)]
_H.mediaway_adts_muxer_close.restype = None
_H.mediaway_adts_muxer_close.argtypes = [c_void_p]

_H.mediaway_adts_demuxer_create.restype = c_void_p
_H.mediaway_adts_demuxer_create.argtypes = []
_H.mediaway_adts_demuxer_push_bytes.restype = c_int32
_H.mediaway_adts_demuxer_push_bytes.argtypes = [c_void_p, U8P, c_size_t]
_H.mediaway_adts_demuxer_stream_count.restype = c_size_t
_H.mediaway_adts_demuxer_stream_count.argtypes = [c_void_p]
_H.mediaway_adts_demuxer_stream_at.restype = c_int32
_H.mediaway_adts_demuxer_stream_at.argtypes = [c_void_p, c_size_t, POINTER(StreamInfo)]
_H.mediaway_adts_demuxer_poll_packet.restype = c_int32
_H.mediaway_adts_demuxer_poll_packet.argtypes = [c_void_p, POINTER(Packet), POINTER(c_bool)]
_H.mediaway_adts_demuxer_close.restype = None
_H.mediaway_adts_demuxer_close.argtypes = [c_void_p]

# ── container.h: FLV (adr/container/0005) ────────────────────────────────────
# flv::Muxer writes directly into a caller-supplied buffer on every call
# instead of buffering for a separate poll_bytes step, and has a fixed
# one-video/one-audio track slot instead of caller-assigned track ids.

_H.mediaway_flv_muxer_create.restype = c_void_p
_H.mediaway_flv_muxer_create.argtypes = []
_H.mediaway_flv_muxer_write_header.restype = c_int32
_H.mediaway_flv_muxer_write_header.argtypes = [c_void_p, c_bool, c_bool, POINTER(U8P), POINTER(c_size_t)]
_H.mediaway_flv_muxer_add_video_track.restype = c_int32
_H.mediaway_flv_muxer_add_video_track.argtypes = [c_void_p, POINTER(VideoTrackInfo)]
_H.mediaway_flv_muxer_add_audio_track.restype = c_int32
_H.mediaway_flv_muxer_add_audio_track.argtypes = [c_void_p, POINTER(AudioTrackInfo)]
_H.mediaway_flv_muxer_push_packet.restype = c_int32
_H.mediaway_flv_muxer_push_packet.argtypes = [c_void_p, POINTER(PacketView), POINTER(U8P), POINTER(c_size_t)]
_H.mediaway_flv_muxer_close.restype = None
_H.mediaway_flv_muxer_close.argtypes = [c_void_p]

_H.mediaway_flv_demuxer_create.restype = c_void_p
_H.mediaway_flv_demuxer_create.argtypes = []
_H.mediaway_flv_demuxer_push_bytes.restype = c_int32
_H.mediaway_flv_demuxer_push_bytes.argtypes = [c_void_p, U8P, c_size_t]
_H.mediaway_flv_demuxer_stream_count.restype = c_size_t
_H.mediaway_flv_demuxer_stream_count.argtypes = [c_void_p]
_H.mediaway_flv_demuxer_stream_at.restype = c_int32
_H.mediaway_flv_demuxer_stream_at.argtypes = [c_void_p, c_size_t, POINTER(StreamInfo)]
_H.mediaway_flv_demuxer_poll_packet.restype = c_int32
_H.mediaway_flv_demuxer_poll_packet.argtypes = [c_void_p, POINTER(Packet), POINTER(c_bool)]
_H.mediaway_flv_demuxer_close.restype = None
_H.mediaway_flv_demuxer_close.argtypes = [c_void_p]

# ── container.h: MPEG-TS (adr/container/0006) ────────────────────────────────
# The full elementary-stream list is fixed at construction (no add_track
# after); write_pat_pmt/write_access_unit write directly into a
# caller-supplied buffer with explicit pts_90k/dts_90k clock values.


class TsElementaryStream(Structure):
    _fields_ = [
        ("pid", c_uint16),
        ("codec", c_int32),
    ]


_H.mediaway_ts_muxer_create.restype = c_void_p
_H.mediaway_ts_muxer_create.argtypes = [c_uint16, c_uint16, POINTER(TsElementaryStream), c_size_t]
_H.mediaway_ts_muxer_write_pat_pmt.restype = c_int32
_H.mediaway_ts_muxer_write_pat_pmt.argtypes = [c_void_p, POINTER(U8P), POINTER(c_size_t)]
_H.mediaway_ts_muxer_write_access_unit.restype = c_int32
_H.mediaway_ts_muxer_write_access_unit.argtypes = [
    c_void_p,
    c_uint16,
    U8P,
    c_size_t,
    c_uint64,
    c_bool,
    c_uint64,
    c_bool,
    POINTER(U8P),
    POINTER(c_size_t),
]
_H.mediaway_ts_muxer_close.restype = None
_H.mediaway_ts_muxer_close.argtypes = [c_void_p]

_H.mediaway_ts_demuxer_create.restype = c_void_p
_H.mediaway_ts_demuxer_create.argtypes = []
_H.mediaway_ts_demuxer_push_bytes.restype = c_int32
_H.mediaway_ts_demuxer_push_bytes.argtypes = [c_void_p, U8P, c_size_t]
_H.mediaway_ts_demuxer_stream_count.restype = c_size_t
_H.mediaway_ts_demuxer_stream_count.argtypes = [c_void_p]
_H.mediaway_ts_demuxer_stream_at.restype = c_int32
_H.mediaway_ts_demuxer_stream_at.argtypes = [c_void_p, c_size_t, POINTER(StreamInfo)]
_H.mediaway_ts_demuxer_poll_packet.restype = c_int32
_H.mediaway_ts_demuxer_poll_packet.argtypes = [c_void_p, POINTER(Packet), POINTER(c_bool)]
_H.mediaway_ts_demuxer_finish.restype = c_int32
_H.mediaway_ts_demuxer_finish.argtypes = [c_void_p, POINTER(POINTER(Packet)), POINTER(c_size_t)]
_H.mediaway_ts_demuxer_finish_free.restype = None
_H.mediaway_ts_demuxer_finish_free.argtypes = [POINTER(Packet), c_size_t]
_H.mediaway_ts_demuxer_close.restype = None
_H.mediaway_ts_demuxer_close.argtypes = [c_void_p]

# ── container.h: MP3 (adr/container/0007) ────────────────────────────────────
# A fixed header for the mux session's lifetime (no track registration at
# all) and write_frame takes an explicit padding bit.

# enum mediaway_mpeg_version
MPEG_VERSION_1 = 0
MPEG_VERSION_2 = 1
MPEG_VERSION_2_5 = 2

# enum mediaway_channel_mode
CHANNEL_MODE_STEREO = 0
CHANNEL_MODE_JOINT_STEREO = 1
CHANNEL_MODE_DUAL_CHANNEL = 2
CHANNEL_MODE_MONO = 3


class Mp3FrameHeader(Structure):
    _fields_ = [
        ("version", c_int32),
        ("bitrate_kbps", c_uint16),
        ("sample_rate", c_uint32),
        ("channel_mode", c_int32),
    ]


_H.mediaway_mp3_muxer_create.restype = c_void_p
_H.mediaway_mp3_muxer_create.argtypes = [POINTER(Mp3FrameHeader)]
_H.mediaway_mp3_muxer_write_frame.restype = c_int32
_H.mediaway_mp3_muxer_write_frame.argtypes = [c_void_p, U8P, c_size_t, c_bool, POINTER(U8P), POINTER(c_size_t)]
_H.mediaway_mp3_muxer_close.restype = None
_H.mediaway_mp3_muxer_close.argtypes = [c_void_p]

_H.mediaway_mp3_demuxer_create.restype = c_void_p
_H.mediaway_mp3_demuxer_create.argtypes = []
_H.mediaway_mp3_demuxer_push_bytes.restype = c_int32
_H.mediaway_mp3_demuxer_push_bytes.argtypes = [c_void_p, U8P, c_size_t]
_H.mediaway_mp3_demuxer_stream_count.restype = c_size_t
_H.mediaway_mp3_demuxer_stream_count.argtypes = [c_void_p]
_H.mediaway_mp3_demuxer_stream_at.restype = c_int32
_H.mediaway_mp3_demuxer_stream_at.argtypes = [c_void_p, c_size_t, POINTER(StreamInfo)]
_H.mediaway_mp3_demuxer_poll_packet.restype = c_int32
_H.mediaway_mp3_demuxer_poll_packet.argtypes = [c_void_p, POINTER(Packet), POINTER(c_bool)]
_H.mediaway_mp3_demuxer_close.restype = None
_H.mediaway_mp3_demuxer_close.argtypes = [c_void_p]

# ── container.h: WAV (adr/container/0008) ────────────────────────────────────
# wav::Muxer::finish consumes self by value, so there is no poll_bytes step.
# Demux has NO handle at all: mediaway_wav_parse is a one-shot function.

# enum mediaway_wav_sample_format — NOT the same as SAMPLE_S16/S32/F32 above
# (raw PCM bit depth); this is the WAVE fmt chunk's wFormatTag encoding.
WAV_SAMPLE_FORMAT_PCM = 0
WAV_SAMPLE_FORMAT_FLOAT = 1


class WaveFormat(Structure):
    _fields_ = [
        ("sample_format", c_int32),
        ("channels", c_uint16),
        ("sample_rate", c_uint32),
        ("bits_per_sample", c_uint16),
    ]


_H.mediaway_wav_muxer_create.restype = c_void_p
_H.mediaway_wav_muxer_create.argtypes = [c_uint32, c_uint16, c_uint16]
_H.mediaway_wav_muxer_create_with_format.restype = c_void_p
_H.mediaway_wav_muxer_create_with_format.argtypes = [POINTER(WaveFormat)]
_H.mediaway_wav_muxer_push_packet.restype = c_int32
_H.mediaway_wav_muxer_push_packet.argtypes = [c_void_p, POINTER(PacketView)]
_H.mediaway_wav_muxer_finish.restype = c_int32
_H.mediaway_wav_muxer_finish.argtypes = [c_void_p, POINTER(U8P), POINTER(c_size_t)]
_H.mediaway_wav_muxer_close.restype = None
_H.mediaway_wav_muxer_close.argtypes = [c_void_p]

_H.mediaway_wav_parse.restype = c_int32
_H.mediaway_wav_parse.argtypes = [U8P, c_size_t, POINTER(StreamInfo), POINTER(Packet)]
