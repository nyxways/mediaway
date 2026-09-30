"""ctypes bindings for `device.h`: camera, screen/window, microphone, loopback and hotplug.

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
from ._ffi_pipeline import *  # noqa: F403  # GPU handle and pixel-format types the capture structs embed

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
DEVICE_REGION_OUT_OF_BOUNDS = 14  # capture region does not fit the surface (adr/0005)

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

# enum mediaway_capture_cursor / mediaway_capture_border / mediaway_frame_dimensions
# (adr/device/0005-window-capture-c-abi.md). Zero is the previous behaviour in every case.
CAPTURE_CURSOR_EXCLUDED = 0
CAPTURE_CURSOR_INCLUDED = 1
CAPTURE_BORDER_SHOWN = 0
CAPTURE_BORDER_HIDDEN = 1
FRAME_DIMENSIONS_NATIVE = 0
FRAME_DIMENSIONS_EVEN_CROPPED = 1

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
    # Every field after `gpu_device` was added by adr/device/0005-window-capture-c-abi.md
    # (device ABI 2) and is zero-means-previous-behaviour.
    _fields_ = [
        ("source_kind", c_int32),
        ("source_index", c_uint32),
        ("time_base", Rational),
        ("gpu_device", GpuDeviceHandle),
        ("window_handle", c_uint64),
        ("cursor", c_int32),
        ("border", c_int32),
        ("dimensions", c_int32),
        ("region_x", c_uint32),
        ("region_y", c_uint32),
        ("region_width", c_uint32),
        ("region_height", c_uint32),
        ("region_enabled", c_bool),
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
        ("include_target_process_tree", c_bool),
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

# ── GPU device factory (adr/0007-gpu-device-factory.md) ────────────────────────

_H.mediaway_gpu_adapter_list.restype = c_int32
_H.mediaway_gpu_adapter_list.argtypes = [POINTER(POINTER(GpuAdapterInfo)), POINTER(c_size_t)]
_H.mediaway_gpu_adapter_list_free.restype = None
_H.mediaway_gpu_adapter_list_free.argtypes = [POINTER(GpuAdapterInfo), c_size_t]
_H.mediaway_gpu_device_create.restype = c_int32
_H.mediaway_gpu_device_create.argtypes = [POINTER(GpuDeviceOptions), POINTER(c_void_p)]
_H.mediaway_gpu_device_handle.restype = c_int32
_H.mediaway_gpu_device_handle.argtypes = [c_void_p, POINTER(GpuDeviceHandle)]
_H.mediaway_gpu_device_close.restype = None
_H.mediaway_gpu_device_close.argtypes = [c_void_p]

# ── Desktop (Screen) ───────────────────────────────────────────────────────────

_H.mediaway_desktop_capture_config_screen.restype = DesktopCaptureConfig
_H.mediaway_desktop_capture_config_screen.argtypes = [c_uint32, Rational, GpuDeviceHandle]
_H.mediaway_desktop_capture_config_window.restype = DesktopCaptureConfig
_H.mediaway_desktop_capture_config_window.argtypes = [c_uint64, Rational, GpuDeviceHandle]
_H.mediaway_desktop_capture_border_hidden.restype = c_int32
_H.mediaway_desktop_capture_border_hidden.argtypes = [c_void_p, POINTER(c_bool)]
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
