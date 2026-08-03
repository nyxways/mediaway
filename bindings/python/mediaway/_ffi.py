"""ctypes bindings over Mediaway's C ABI (mediaway-*-ffi).

This module is the raw ABI layer: struct layouts and function prototypes,
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

__all__ = [
    "container",
    "pipeline",
    "device",
    "lib_dir",
]


# ── Library discovery ────────────────────────────────────────────────────────
#
# The cdylibs are Rust build artifacts, not installed system libraries. We look
# in, in order:
#   1. $MEDIAWAY_FFI_DIR
#   2. <package>/_native/                          (DLLs bundled in the wheel — the PyPI distribution)
#   3. <repo root>/target/x86_64-pc-windows-gnu/debug   (GNU toolchain, C examples)
#   4. <repo root>/target/debug                          (host/MSVC toolchain, C# tests)
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


container = _Library("mediaway_ffi.dll")
pipeline = _Library("mediaway_ffi.dll")
device = _Library("mediaway_ffi.dll")


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


# ── pipeline.h: status codes ──────────────────────────────────────────────────

PIPELINE_OK = 0
PIPELINE_INVALID_ARGUMENT = 1
PIPELINE_HANDLE_POISONED = 2
PIPELINE_NO_BACKEND = 3
PIPELINE_UNSUPPORTED = 4
PIPELINE_INVALID_INPUT = 5
PIPELINE_ENCODER_BACKEND_FAILURE = 6
PIPELINE_ENCODER_CLOSED = 7
PIPELINE_MUX_INVALID_TRACK = 8
PIPELINE_MUX_INVALID_PACKET = 9
PIPELINE_MUX_INVALID_DATA = 10
PIPELINE_UNKNOWN_ERROR = 11
PIPELINE_INTERNAL_PANIC = 12

# enum mediaway_pixel_format
PIXEL_NV12 = 0
PIXEL_I420 = 1
PIXEL_BGRA8 = 2
PIXEL_RGBA8 = 3
PIXEL_YUYV = 4

# enum mediaway_gpu_device_kind
GPU_DEVICE_NONE = 0
GPU_DEVICE_DIRECTX11 = 1
GPU_DEVICE_DIRECTX12 = 2
GPU_DEVICE_VULKAN = 3
GPU_DEVICE_METAL = 4
GPU_DEVICE_WEBGPU = 5

# enum mediaway_gpu_buffer_kind
GPU_BUFFER_DIRECTX11 = 0
GPU_BUFFER_DIRECTX12 = 1
GPU_BUFFER_DIRECTX_SHARED = 2
GPU_BUFFER_METAL = 3
GPU_BUFFER_ANDROID_SURFACE = 4
GPU_BUFFER_VULKAN = 5
GPU_BUFFER_WEBGPU = 6
GPU_BUFFER_UNKNOWN = 255

# enum mediaway_video_frame_storage_kind
STORAGE_CPU = 0
STORAGE_GPU = 1


class GpuDeviceHandle(Structure):
    _fields_ = [
        ("kind", c_int32),
        ("native", c_size_t),  # uintptr_t
        ("webgpu_device_id", c_uint64),
    ]


class GpuBufferHandle(Structure):
    _fields_ = [
        ("kind", c_int32),
        ("native_a", c_size_t),  # uintptr_t
        ("native_b", c_size_t),  # uintptr_t
        ("subresource", c_uint32),
        ("webgpu_texture_id", c_uint64),
    ]


class AutoVideoEncodeConfig(Structure):
    _fields_ = [
        ("codec", c_int32),
        ("width", c_uint32),
        ("height", c_uint32),
        ("time_base", Rational),
        ("bitrate_bps", c_uint32),
        ("pixel_format", c_int32),
        ("gpu_device", GpuDeviceHandle),
    ]


class VideoFrame(Structure):  # borrowed input
    _fields_ = [
        ("pts", c_int64),
        ("duration", c_uint64),
        ("width", c_uint32),
        ("height", c_uint32),
        ("pixel_format", c_int32),
        ("storage_kind", c_int32),
        ("raw_bytes", U8P),  # CPU only, borrowed
        ("raw_bytes_len", c_size_t),
        ("gpu_buffer", GpuBufferHandle),  # GPU only, borrowed
    ]


_H = pipeline.dll

_H.mediaway_pipeline_ffi_abi_version.restype = c_uint32
_H.mediaway_pipeline_ffi_abi_version.argtypes = []

_H.mediaway_auto_video_encode_config_new.restype = AutoVideoEncodeConfig
_H.mediaway_auto_video_encode_config_new.argtypes = [c_int32, c_uint32, c_uint32, Rational]
_H.mediaway_auto_video_encode_config_h264.restype = AutoVideoEncodeConfig
_H.mediaway_auto_video_encode_config_h264.argtypes = [c_uint32, c_uint32, Rational]

_H.mediaway_auto_encoder_open.restype = c_int32
_H.mediaway_auto_encoder_open.argtypes = [POINTER(AutoVideoEncodeConfig), POINTER(c_void_p)]
_H.mediaway_auto_encoder_close.restype = None
_H.mediaway_auto_encoder_close.argtypes = [c_void_p]

_H.mediaway_encode_session_open.restype = c_int32
_H.mediaway_encode_session_open.argtypes = [c_void_p, POINTER(c_void_p)]
_H.mediaway_encode_session_write_frame.restype = c_int32
_H.mediaway_encode_session_write_frame.argtypes = [c_void_p, POINTER(VideoFrame)]
_H.mediaway_encode_session_finish.restype = c_int32
_H.mediaway_encode_session_finish.argtypes = [c_void_p, POINTER(U8P), POINTER(c_size_t)]
_H.mediaway_encode_session_close.restype = None
_H.mediaway_encode_session_close.argtypes = [c_void_p]

_H.mediaway_pipeline_ffi_buffer_free.restype = None
_H.mediaway_pipeline_ffi_buffer_free.argtypes = [U8P, c_size_t]

# ── pipeline.h: audio encode (ABI v2, adr/0003) ──────────────────────────────

# enum mediaway_sample_format — same values as device.h's (S16=0, S32=1, F32=2)


class AudioEncodeConfig(Structure):
    _fields_ = [
        ("codec", c_int32),
        ("sample_rate", c_uint32),
        ("channels", c_uint16),
        ("sample_format", c_int32),
        ("time_base", Rational),
        ("bitrate_bps", c_uint32),
    ]


class AudioFrameView(Structure):  # borrowed input
    _fields_ = [
        ("pts", c_int64),
        ("duration", c_uint64),
        ("sample_rate", c_uint32),
        ("channels", c_uint16),
        ("sample_format", c_int32),
        ("data", U8P),  # borrowed
        ("data_len", c_size_t),
    ]


class AudioPacket(Structure):  # owned output
    _fields_ = [
        ("pts", c_int64),
        ("dts", c_int64),
        ("duration", c_uint64),
        ("is_keyframe", c_bool),
        ("is_discard", c_bool),
        ("payload", U8P),  # owned
        ("payload_len", c_size_t),
    ]


class AudioStreamInfo(Structure):  # owned output
    _fields_ = [
        ("codec", c_int32),
        ("time_base", Rational),
        ("sample_rate", c_uint32),
        ("channels", c_uint16),
        ("extra_data", U8P),  # owned
        ("extra_data_len", c_size_t),
    ]


_H.mediaway_audio_encode_config_aac.restype = AudioEncodeConfig
_H.mediaway_audio_encode_config_aac.argtypes = [c_uint32, Rational]

_H.mediaway_audio_encoder_open.restype = c_int32
_H.mediaway_audio_encoder_open.argtypes = [POINTER(AudioEncodeConfig), POINTER(c_void_p)]

_H.mediaway_audio_encode_session_push_pcm.restype = c_int32
_H.mediaway_audio_encode_session_push_pcm.argtypes = [c_void_p, POINTER(AudioFrameView)]

_H.mediaway_audio_encode_session_poll_packet.restype = c_int32
_H.mediaway_audio_encode_session_poll_packet.argtypes = [c_void_p, POINTER(AudioPacket), POINTER(c_bool)]

_H.mediaway_audio_encode_session_flush.restype = c_int32
_H.mediaway_audio_encode_session_flush.argtypes = [c_void_p]

_H.mediaway_audio_encode_session_stream_info.restype = c_int32
_H.mediaway_audio_encode_session_stream_info.argtypes = [c_void_p, POINTER(AudioStreamInfo)]

_H.mediaway_audio_encode_session_close.restype = None
_H.mediaway_audio_encode_session_close.argtypes = [c_void_p]

_H.mediaway_pipeline_ffi_packet_free.restype = None
_H.mediaway_pipeline_ffi_packet_free.argtypes = [POINTER(AudioPacket)]

_H.mediaway_pipeline_ffi_stream_info_free.restype = None
_H.mediaway_pipeline_ffi_stream_info_free.argtypes = [POINTER(AudioStreamInfo)]


# ── device.h: status codes ────────────────────────────────────────────────────

DEVICE_OK = 0
DEVICE_INVALID_ARGUMENT = 1
DEVICE_HANDLE_POISONED = 2
DEVICE_UNSUPPORTED = 3
DEVICE_NO_BACKEND = 4
DEVICE_INVALID_INPUT = 5
DEVICE_BACKEND_FAILURE = 6
DEVICE_CLOSED = 7
DEVICE_ACCESS_DENIED = 8
DEVICE_UNKNOWN_ERROR = 9
DEVICE_INTERNAL_PANIC = 10
DEVICE_CALLBACK_ALREADY_REGISTERED = 11
DEVICE_CALLBACK_MODE_ACTIVE = 12
DEVICE_TIMEOUT = 13

# enum mediaway_sample_format
SAMPLE_S16 = 0
SAMPLE_S32 = 1
SAMPLE_F32 = 2

# enum mediaway_device_kind (hotplug)
DEVKIND_SCREEN = 0
DEVKIND_WINDOW = 1
DEVKIND_CAMERA = 2
DEVKIND_MICROPHONE = 3
DEVKIND_LOOPBACK = 4
DEVKIND_PROCESS_LOOPBACK = 5
DEVKIND_UNKNOWN = 255

# enum mediaway_desktop_capture_source_kind
DESKTOP_SOURCE_SCREEN = 0
DESKTOP_SOURCE_WINDOW = 1

# enum mediaway_desktop_audio_source_kind
DESKTOP_AUDIO_LOOPBACK = 0
DESKTOP_AUDIO_PROCESS_LOOPBACK = 1


class CameraCaptureConfig(Structure):
    _fields_ = [
        ("device_index", c_uint32),
        ("time_base", Rational),
    ]


class CameraFrame(Structure):  # owned output; CPU-only
    _fields_ = [
        ("pts", c_int64),
        ("duration", c_uint64),
        ("width", c_uint32),
        ("height", c_uint32),
        ("pixel_format", c_int32),
        ("data", U8P),  # owned
        ("data_len", c_size_t),
    ]


class DesktopCaptureConfig(Structure):
    _fields_ = [
        ("source_kind", c_int32),
        ("source_index", c_uint32),
        ("time_base", Rational),
        ("gpu_device", GpuDeviceHandle),
    ]


class DesktopFrame(Structure):  # owned output (CPU) / borrowed (GPU)
    _fields_ = [
        ("pts", c_int64),
        ("duration", c_uint64),
        ("width", c_uint32),
        ("height", c_uint32),
        ("pixel_format", c_int32),
        ("storage_kind", c_int32),
        ("data", U8P),  # CPU only, owned
        ("data_len", c_size_t),
        ("gpu_buffer", GpuBufferHandle),  # GPU only, borrowed
    ]


class AudioCaptureConfig(Structure):
    _fields_ = [
        ("device_index", c_uint32),
        ("time_base", Rational),
        ("sample_format", c_int32),
    ]


class DeviceAudioFrame(Structure):  # owned output
    _fields_ = [
        ("pts", c_int64),
        ("duration", c_uint64),
        ("sample_rate", c_uint32),
        ("channels", c_uint16),
        ("sample_format", c_int32),
        ("data", U8P),  # owned
        ("data_len", c_size_t),
    ]


class DesktopAudioCaptureConfig(Structure):
    _fields_ = [
        ("source_kind", c_int32),
        ("device_index", c_uint32),
        ("process_id", c_uint32),
        ("include_child_processes", c_bool),
        ("time_base", Rational),
        ("sample_format", c_int32),
    ]


class DesktopAudioFrame(Structure):  # owned output
    _fields_ = [
        ("pts", c_int64),
        ("duration", c_uint64),
        ("sample_rate", c_uint32),
        ("channels", c_uint16),
        ("sample_format", c_int32),
        ("data", U8P),  # owned
        ("data_len", c_size_t),
    ]


class DeviceEvent(Structure):  # owned via poll_event; borrowed via callback
    _fields_ = [
        ("event_kind", c_int32),
        ("device_kind", c_int32),
        ("device_id", c_char_p),  # owned NUL-terminated UTF-8
    ]


HOTPLUG_CALLBACK = _c.CFUNCTYPE(None, c_void_p, POINTER(DeviceEvent))

_H = device.dll

_H.mediaway_device_ffi_abi_version.restype = c_uint32
_H.mediaway_device_ffi_abi_version.argtypes = []

# ── Camera ────────────────────────────────────────────────────────────────────

_H.mediaway_camera_capture_config_default.restype = CameraCaptureConfig
_H.mediaway_camera_capture_config_default.argtypes = [c_uint32, Rational]
_H.mediaway_camera_capture_open.restype = c_int32
_H.mediaway_camera_capture_open.argtypes = [POINTER(CameraCaptureConfig), POINTER(c_void_p)]
_H.mediaway_camera_capture_geometry.restype = c_int32
_H.mediaway_camera_capture_geometry.argtypes = [c_void_p, POINTER(c_uint32), POINTER(c_uint32)]
_H.mediaway_camera_capture_poll_frame.restype = c_int32
_H.mediaway_camera_capture_poll_frame.argtypes = [c_void_p, POINTER(CameraFrame), POINTER(c_bool)]
_H.mediaway_camera_capture_poll_frame_blocking.restype = c_int32
_H.mediaway_camera_capture_poll_frame_blocking.argtypes = [c_void_p, c_uint32, POINTER(CameraFrame)]
_H.mediaway_camera_capture_capture_once.restype = c_int32
_H.mediaway_camera_capture_capture_once.argtypes = [POINTER(CameraCaptureConfig), c_uint32, POINTER(CameraFrame)]
_H.mediaway_camera_capture_release_frame.restype = c_int32
_H.mediaway_camera_capture_release_frame.argtypes = [c_void_p]
_H.mediaway_camera_capture_close.restype = c_int32
_H.mediaway_camera_capture_close.argtypes = [c_void_p]
_H.mediaway_camera_frame_free.restype = None
_H.mediaway_camera_frame_free.argtypes = [POINTER(CameraFrame)]

# ── Desktop (Screen) ───────────────────────────────────────────────────────────

_H.mediaway_desktop_capture_config_screen.restype = DesktopCaptureConfig
_H.mediaway_desktop_capture_config_screen.argtypes = [c_uint32, Rational, GpuDeviceHandle]
_H.mediaway_desktop_capture_open.restype = c_int32
_H.mediaway_desktop_capture_open.argtypes = [POINTER(DesktopCaptureConfig), POINTER(c_void_p)]
_H.mediaway_desktop_capture_geometry.restype = c_int32
_H.mediaway_desktop_capture_geometry.argtypes = [c_void_p, POINTER(c_uint32), POINTER(c_uint32)]
_H.mediaway_desktop_capture_poll_frame.restype = c_int32
_H.mediaway_desktop_capture_poll_frame.argtypes = [c_void_p, POINTER(DesktopFrame), POINTER(c_bool)]
_H.mediaway_desktop_capture_poll_frame_blocking.restype = c_int32
_H.mediaway_desktop_capture_poll_frame_blocking.argtypes = [c_void_p, c_uint32, POINTER(DesktopFrame)]
_H.mediaway_desktop_capture_release_frame.restype = c_int32
_H.mediaway_desktop_capture_release_frame.argtypes = [c_void_p]
_H.mediaway_desktop_capture_close.restype = c_int32
_H.mediaway_desktop_capture_close.argtypes = [c_void_p]
_H.mediaway_desktop_frame_free.restype = None
_H.mediaway_desktop_frame_free.argtypes = [POINTER(DesktopFrame)]

# ── Audio (Microphone) ─────────────────────────────────────────────────────────

_H.mediaway_audio_capture_config_microphone.restype = AudioCaptureConfig
_H.mediaway_audio_capture_config_microphone.argtypes = [Rational]
_H.mediaway_audio_capture_open.restype = c_int32
_H.mediaway_audio_capture_open.argtypes = [POINTER(AudioCaptureConfig), POINTER(c_void_p)]
_H.mediaway_audio_capture_format.restype = c_int32
_H.mediaway_audio_capture_format.argtypes = [c_void_p, POINTER(c_uint32), POINTER(c_uint16)]
_H.mediaway_audio_capture_poll_frame.restype = c_int32
_H.mediaway_audio_capture_poll_frame.argtypes = [c_void_p, POINTER(DeviceAudioFrame), POINTER(c_bool)]
_H.mediaway_audio_capture_close.restype = c_int32
_H.mediaway_audio_capture_close.argtypes = [c_void_p]
_H.mediaway_audio_frame_free.restype = None
_H.mediaway_audio_frame_free.argtypes = [POINTER(DeviceAudioFrame)]

# ── Desktop audio (Loopback / ProcessLoopback) ────────────────────────────────

_H.mediaway_desktop_audio_capture_config_loopback.restype = DesktopAudioCaptureConfig
_H.mediaway_desktop_audio_capture_config_loopback.argtypes = [Rational]
_H.mediaway_desktop_audio_capture_config_process_loopback.restype = DesktopAudioCaptureConfig
_H.mediaway_desktop_audio_capture_config_process_loopback.argtypes = [c_uint32, c_bool, Rational]
_H.mediaway_desktop_audio_capture_open.restype = c_int32
_H.mediaway_desktop_audio_capture_open.argtypes = [POINTER(DesktopAudioCaptureConfig), POINTER(c_void_p)]
_H.mediaway_desktop_audio_capture_format.restype = c_int32
_H.mediaway_desktop_audio_capture_format.argtypes = [c_void_p, POINTER(c_uint32), POINTER(c_uint16)]
_H.mediaway_desktop_audio_capture_poll_frame.restype = c_int32
_H.mediaway_desktop_audio_capture_poll_frame.argtypes = [c_void_p, POINTER(DesktopAudioFrame), POINTER(c_bool)]
_H.mediaway_desktop_audio_capture_close.restype = c_int32
_H.mediaway_desktop_audio_capture_close.argtypes = [c_void_p]
_H.mediaway_desktop_audio_frame_free.restype = None
_H.mediaway_desktop_audio_frame_free.argtypes = [POINTER(DesktopAudioFrame)]

# ── Hotplug ────────────────────────────────────────────────────────────────────

_H.mediaway_device_hotplug_open.restype = c_int32
_H.mediaway_device_hotplug_open.argtypes = [POINTER(c_int32), c_size_t, POINTER(c_void_p)]
_H.mediaway_device_hotplug_close.restype = c_int32
_H.mediaway_device_hotplug_close.argtypes = [c_void_p]
_H.mediaway_device_hotplug_register_callback.restype = c_int32
_H.mediaway_device_hotplug_register_callback.argtypes = [c_void_p, HOTPLUG_CALLBACK, c_void_p]
_H.mediaway_device_hotplug_unregister_callback.restype = c_int32
_H.mediaway_device_hotplug_unregister_callback.argtypes = [c_void_p]
_H.mediaway_device_hotplug_poll_event.restype = c_int32
_H.mediaway_device_hotplug_poll_event.argtypes = [c_void_p, POINTER(DeviceEvent), POINTER(c_bool)]
_H.mediaway_device_hotplug_event_free.restype = None
_H.mediaway_device_hotplug_event_free.argtypes = [POINTER(DeviceEvent)]
