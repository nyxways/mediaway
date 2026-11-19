"""ctypes bindings for `pipeline.h`: encode, decode, GPU device factory and capability probes.

Part of the raw ABI layer that `mediaway/_ffi.py` re-exports as one namespace.
"""

from __future__ import annotations

import ctypes as _c
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

from ._ffi_base import *  # noqa: F403  # shared value types, library handles, container constants
from ._ffi_base import U8P, Rational, c_ubyte  # noqa: F401

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


# ── GPU device factory (device.h, mediaway-device ADR-0007) ─────────────────

# enum mediaway_gpu_adapter_select_kind
GPU_ADAPTER_SELECT_DEFAULT = 0
GPU_ADAPTER_SELECT_INDEX = 1


class GpuAdapterInfo(Structure):  # owned output (one array entry)
    _fields_ = [
        ("index", c_uint32),
        ("name", c_char_p),  # owned NUL-terminated UTF-8; freed only via the array free below
        ("vendor_id", c_uint32),
        ("device_id", c_uint32),
        ("dedicated_video_memory", c_uint64),
        ("is_hardware", c_bool),
    ]


class GpuAdapterSelect(Structure):
    _fields_ = [
        ("kind", c_int32),
        ("index", c_uint32),  # meaningful only when kind == GPU_ADAPTER_SELECT_INDEX
    ]


class GpuDeviceOptions(Structure):
    _fields_ = [
        ("adapter", GpuAdapterSelect),
        ("video_support", c_bool),  # D3D11_CREATE_DEVICE_VIDEO_SUPPORT
        ("debug_layer", c_bool),  # D3D11_CREATE_DEVICE_DEBUG
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

# MEDIAWAY_PIPELINE_FFI_ABI_VERSION this binding's struct mirrors were written against.
# 7: mediaway_audio_decode_config_t grew extra_data/extra_data_len (AAC decode), plus
# mediaway_encode_session_poll_bytes and the capability probes
# (adr/pipeline/0007-stream-bytes-aac-decode-support-probe.md).
PIPELINE_ABI_VERSION = 7

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
# adr/pipeline/0007 §1: owned buffer (free with mediaway_pipeline_ffi_buffer_free); NULL/0 when idle.
_H.mediaway_encode_session_poll_bytes.restype = c_int32
_H.mediaway_encode_session_poll_bytes.argtypes = [c_void_p, POINTER(U8P), POINTER(c_size_t)]
_H.mediaway_encode_session_close.restype = None
_H.mediaway_encode_session_close.argtypes = [c_void_p]

_H.mediaway_pipeline_ffi_buffer_free.restype = None
_H.mediaway_pipeline_ffi_buffer_free.argtypes = [U8P, c_size_t]

# ── pipeline.h: capture-to-encode bridge (adr/pipeline/0005) ────────────────
# session/capture are both opaque native pointers — poll-and-push in one call,
# no intermediate mediaway_video_frame_t, Zero-Copy for GPU-backed frames.

_H.mediaway_encode_session_write_frame_from_camera_capture.restype = c_int32
_H.mediaway_encode_session_write_frame_from_camera_capture.argtypes = [c_void_p, c_void_p, POINTER(c_bool)]
_H.mediaway_encode_session_write_frame_from_desktop_capture.restype = c_int32
_H.mediaway_encode_session_write_frame_from_desktop_capture.argtypes = [c_void_p, c_void_p, POINTER(c_bool)]

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


# ── pipeline.h: decode (adr/0004-auto-decode-c-abi.md, adr/pipeline/0006) ───
#
# mediaway_decode_packet_view_t is shared by both video and audio decode
# push_packet calls — a new, pipeline-scoped type, not container.h's
# PacketView above.

PIPELINE_DECODER_BACKEND_FAILURE = 13
PIPELINE_DECODER_CLOSED = 14


class DecodePacketView(Structure):  # borrowed input
    _fields_ = [
        ("stream_id", c_uint32),  # unused by decode; kept for call-site symmetry
        ("pts", c_int64),
        ("dts", c_int64),
        ("duration", c_uint64),
        ("is_keyframe", c_bool),
        ("is_discard", c_bool),
        ("payload", U8P),  # borrowed
        ("payload_len", c_size_t),
    ]


class AutoVideoDecodeConfig(Structure):
    _fields_ = [
        ("codec", c_int32),
        ("width", c_uint32),
        ("height", c_uint32),
        ("time_base", Rational),
        ("pixel_format", c_int32),
        ("extra_data", U8P),  # borrowed, open-call only
        ("extra_data_len", c_size_t),
    ]


class DecodedVideoFrame(Structure):  # owned output
    _fields_ = [
        ("pts", c_int64),
        ("duration", c_uint64),
        ("width", c_uint32),
        ("height", c_uint32),
        ("pixel_format", c_int32),
        ("data", U8P),  # owned
        ("data_len", c_size_t),
    ]


class AudioDecodeConfig(Structure):
    # Layout pinned against the real header with a gcc offsetof probe (size 48; extra_data at
    # 32, extra_data_len at 40) — tests/test_stream_aac_probe.py.
    _fields_ = [
        ("codec", c_int32),  # Opus or AAC
        ("sample_rate", c_uint32),
        ("channels", c_uint16),
        ("time_base", Rational),
        ("extra_data", U8P),  # AAC: BORROWED AudioSpecificConfig, valid for open() only
        ("extra_data_len", c_size_t),
    ]


class DecodedAudioFrame(Structure):  # owned output
    _fields_ = [
        ("pts", c_int64),
        ("duration", c_uint64),
        ("sample_rate", c_uint32),
        ("channels", c_uint16),
        ("sample_format", c_int32),  # always F32 for Opus
        ("data", U8P),  # owned interleaved PCM
        ("data_len", c_size_t),
    ]


_H.mediaway_auto_video_decode_config_new.restype = AutoVideoDecodeConfig
_H.mediaway_auto_video_decode_config_new.argtypes = [c_int32, c_uint32, c_uint32, Rational, U8P, c_size_t]

_H.mediaway_decode_session_open.restype = c_int32
_H.mediaway_decode_session_open.argtypes = [POINTER(AutoVideoDecodeConfig), POINTER(c_void_p)]
_H.mediaway_decode_session_push_packet.restype = c_int32
_H.mediaway_decode_session_push_packet.argtypes = [c_void_p, POINTER(DecodePacketView)]
_H.mediaway_decode_session_poll_frame.restype = c_int32
_H.mediaway_decode_session_poll_frame.argtypes = [c_void_p, POINTER(DecodedVideoFrame), POINTER(c_bool)]
_H.mediaway_decode_session_flush.restype = c_int32
_H.mediaway_decode_session_flush.argtypes = [c_void_p]
_H.mediaway_decode_session_close.restype = None
_H.mediaway_decode_session_close.argtypes = [c_void_p]
_H.mediaway_decoded_video_frame_free.restype = None
_H.mediaway_decoded_video_frame_free.argtypes = [POINTER(DecodedVideoFrame)]

_H.mediaway_audio_decode_config_opus.restype = AudioDecodeConfig
_H.mediaway_audio_decode_config_opus.argtypes = [c_uint32, c_uint16, Rational]
_H.mediaway_audio_decode_config_aac.restype = AudioDecodeConfig
_H.mediaway_audio_decode_config_aac.argtypes = [c_uint32, c_uint16, Rational, U8P, c_size_t]

_H.mediaway_audio_decode_session_open.restype = c_int32
_H.mediaway_audio_decode_session_open.argtypes = [POINTER(AudioDecodeConfig), POINTER(c_void_p)]
_H.mediaway_audio_decode_session_push_packet.restype = c_int32
_H.mediaway_audio_decode_session_push_packet.argtypes = [c_void_p, POINTER(DecodePacketView)]
_H.mediaway_audio_decode_session_poll_frame.restype = c_int32
_H.mediaway_audio_decode_session_poll_frame.argtypes = [c_void_p, POINTER(DecodedAudioFrame), POINTER(c_bool)]
_H.mediaway_audio_decode_session_flush.restype = c_int32
_H.mediaway_audio_decode_session_flush.argtypes = [c_void_p]
_H.mediaway_audio_decode_session_close.restype = None
_H.mediaway_audio_decode_session_close.argtypes = [c_void_p]
_H.mediaway_decoded_audio_frame_free.restype = None
_H.mediaway_decoded_audio_frame_free.argtypes = [POINTER(DecodedAudioFrame)]


# ── pipeline.h: capability probes (adr/pipeline/0007 §3) ─────────────────────

ENCODE_BACKEND_OS = 0
ENCODE_BACKEND_NVENC = 1
ENCODE_BACKEND_QUICKSYNC = 2
ENCODE_BACKEND_AMF = 3
ENCODE_BACKEND_VULKAN = 4
ENCODE_BACKEND_SOFTWARE = 5
ENCODE_BACKEND_UNKNOWN = 255

SUPPORT_STATE_SUPPORTED = 0
SUPPORT_STATE_NOT_IMPLEMENTED = 1
SUPPORT_STATE_NO_DEVICE = 2
SUPPORT_STATE_UNKNOWN = 255

ENCODE_PATH_NONE = 0
ENCODE_PATH_ZERO_COPY = 1
ENCODE_PATH_GPU_COPY = 2
ENCODE_PATH_CPU_UPLOAD = 3
ENCODE_PATH_READBACK = 4
ENCODE_PATH_SOFTWARE = 5
ENCODE_PATH_UNKNOWN = 255


class EncoderCapability(Structure):  # plain value, no owned fields (size 12)
    _fields_ = [
        ("backend", c_int32),
        ("state", c_int32),
        ("path_class", c_int32),  # meaningful only when state == SUPPORTED
    ]


_H.mediaway_encoder_support_at.restype = c_int32
_H.mediaway_encoder_support_at.argtypes = [
    c_int32,
    c_uint32,
    c_uint32,
    POINTER(POINTER(EncoderCapability)),
    POINTER(c_size_t),
]
_H.mediaway_encoder_support_free.restype = None
_H.mediaway_encoder_support_free.argtypes = [POINTER(EncoderCapability), c_size_t]
_H.mediaway_decoder_support.restype = c_int32
_H.mediaway_decoder_support.argtypes = [c_int32, POINTER(c_int32)]
