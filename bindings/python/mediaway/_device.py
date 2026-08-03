"""Device capability: camera / screen / microphone capture.

Wraps the `mediaway-device-ffi` C ABI (see `_ffi.py` for the raw layer and
`../README.md` for the DX contract). The ABI is domain-split
(`adr/0004-domain-feature-split.md`): Camera (`mediaway_camera_capture_*`),
Desktop/Screen (`mediaway_desktop_capture_*`), Microphone
(`mediaway_audio_capture_*`), and Desktop audio / loopback
(`mediaway_desktop_audio_capture_*`). This module's public API folds the split
back into the two DX classes, `VideoCapture` and `AudioCapture`.

CPU-storage only: Camera delivers owned CPU frames; Screen capture raises
`CaptureUnsupportedError` from the C ABI today (it needs a live GPU device
handle, and there is no CPU fallback). Audio capture delivers raw interleaved
PCM (`poll_pcm`), not encoded audio — there is no audio encoder in the ABI.
"""

from __future__ import annotations

from ctypes import byref, c_bool, c_uint16, c_uint32, c_void_p

from . import _ffi
from ._container import _from_units
from ._errors import CaptureUnsupportedError, DeviceUnavailableError, MediawayError
from ._types import PixelFormat, Rational, VideoFrame

__all__ = ["VideoCapture", "AudioCapture"]


def _check_device(status: int) -> None:
    if status == _ffi.DEVICE_OK:
        return
    if status in (_ffi.DEVICE_NO_BACKEND, _ffi.DEVICE_BACKEND_FAILURE, _ffi.DEVICE_ACCESS_DENIED):
        raise DeviceUnavailableError(status, "no capture backend or device available")
    if status == _ffi.DEVICE_UNSUPPORTED:
        raise CaptureUnsupportedError(status, "this capture configuration is unsupported by the ABI")
    names = {
        _ffi.DEVICE_INVALID_ARGUMENT: "invalid argument",
        _ffi.DEVICE_HANDLE_POISONED: "handle poisoned by an earlier panic",
        _ffi.DEVICE_INVALID_INPUT: "bad capture config",
        _ffi.DEVICE_CLOSED: "session already closed or not open",
        _ffi.DEVICE_UNKNOWN_ERROR: "unknown error",
        _ffi.DEVICE_INTERNAL_PANIC: "internal panic (handle poisoned)",
        _ffi.DEVICE_CALLBACK_ALREADY_REGISTERED: "callback already registered",
        _ffi.DEVICE_CALLBACK_MODE_ACTIVE: "callback mode active (poll disabled)",
        _ffi.DEVICE_TIMEOUT: "timed out waiting for a frame",
    }
    raise MediawayError(status, names.get(status, "unknown status"))


def _read(ptr, length: int) -> bytes:
    import ctypes

    if not ptr or length == 0:
        return b""
    return ctypes.string_at(ptr, length)


class VideoCapture:
    """A video capture session. `source="camera"` opens the Camera ABI (real,
    CPU frames); `source="screen"`/`"window"` surface the documented ABI gap:
    Desktop capture requires a live GPU device handle with no C representation
    yet, so opening raises CaptureUnsupportedError."""

    def __init__(self, handle: int, time_base: Rational):
        self._handle = handle
        self._time_base = time_base

    @classmethod
    def open(
        cls,
        source: str = "camera",
        index: int = 0,
        frame_rate: Rational = Rational(1, 30),
    ) -> "VideoCapture":
        dll = _ffi.device.dll
        tb = _ffi.Rational(frame_rate.num, frame_rate.den)
        out = c_void_p()
        if source == "camera":
            config = dll.mediaway_camera_capture_config_default(index, tb)
            _check_device(dll.mediaway_camera_capture_open(byref(config), byref(out)))
        elif source == "screen":
            # Desktop/Screen needs a live DX11 GPU device handle (no CPU
            # fallback); its C representation is deferred, so the only
            # C-constructible config (NONE device) is rejected with
            # INVALID_INPUT — surface the documented gap, not a generic error.
            config = dll.mediaway_desktop_capture_config_screen(
                index, tb, _ffi.GpuDeviceHandle(kind=_ffi.GPU_DEVICE_NONE, native=0, webgpu_device_id=0)
            )
            try:
                _check_device(dll.mediaway_desktop_capture_open(byref(config), byref(out)))
            except MediawayError as err:
                raise CaptureUnsupportedError(
                    err.status,
                    "Screen capture needs a live GPU device handle (ID3D11Device*) with "
                    "no CPU fallback, and its C representation is deferred — not "
                    "available from this binding today",
                ) from None
        elif source == "window":
            config = dll.mediaway_desktop_capture_config_screen(
                index, tb, _ffi.GpuDeviceHandle(kind=_ffi.GPU_DEVICE_NONE, native=0, webgpu_device_id=0)
            )
            config.source_kind = _ffi.DESKTOP_SOURCE_WINDOW
            try:
                _check_device(dll.mediaway_desktop_capture_open(byref(config), byref(out)))
            except MediawayError as err:
                raise CaptureUnsupportedError(err.status, "Window capture has no C constructor this pass") from None
        else:
            raise ValueError(f"unknown video source: {source!r} (camera | screen | window)")
        if not out.value:
            raise MediawayError(_ffi.DEVICE_UNKNOWN_ERROR, "capture open returned no handle")
        return cls(out.value, frame_rate)

    def size(self) -> tuple[int, int]:
        """The negotiated frame width/height (do not assume a resolution)."""
        dll = _ffi.device.dll
        w = c_uint32(0)
        h = c_uint32(0)
        _check_device(dll.mediaway_camera_capture_geometry(self._handle, byref(w), byref(h)))
        return w.value, h.value

    def frame_rate(self) -> Rational:
        """The configured frame period (the ABI does not re-negotiate it)."""
        return self._time_base

    def poll_frame(self, timeout: float | None = None) -> VideoFrame | None:
        """Poll the next frame; None when nothing is ready yet. With a
        `timeout` (seconds), blocks until a frame or the deadline."""
        dll = _ffi.device.dll
        raw = _ffi.CameraFrame()
        if timeout is None:
            has = c_bool(False)
            _check_device(dll.mediaway_camera_capture_poll_frame(self._handle, byref(raw), byref(has)))
            if not has.value:
                return None
        else:
            status = dll.mediaway_camera_capture_poll_frame_blocking(
                self._handle, int(timeout * 1000), byref(raw)
            )
            if status == _ffi.DEVICE_TIMEOUT:
                return None
            _check_device(status)
        try:
            return VideoFrame(
                width=raw.width,
                height=raw.height,
                format=PixelFormat(raw.pixel_format),
                data=_read(raw.data, raw.data_len),
                pts=_from_units(raw.pts, self._time_base),
                duration=_from_units(raw.duration, self._time_base) if raw.duration else None,
            )
        finally:
            dll.mediaway_camera_frame_free(byref(raw))

    def release_frame(self) -> None:
        """Release backend resources held by the last polled frame. Documented
        no-op for Camera today, but still required before the next poll."""
        _check_device(_ffi.device.dll.mediaway_camera_capture_release_frame(self._handle))

    def close(self) -> None:
        if self._handle:
            # Blocks up to one frame interval (joins the backend worker thread).
            _check_device(_ffi.device.dll.mediaway_camera_capture_close(self._handle))
            self._handle = None

    def __enter__(self) -> "VideoCapture":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()


class AudioCapture:
    """An audio capture session. `source="microphone"` opens the Microphone
    ABI; `"loopback"`/`"process_loopback"` open the Desktop-audio ABI (capture
    of what the desktop is rendering)."""

    def __init__(self, handle: int):
        self._handle = handle
        self._desktop = False
        self._rate: int | None = None
        self._channels: int | None = None

    @classmethod
    def open(
        cls,
        source: str = "microphone",
        sample_rate: int = 48000,
        process_id: int | None = None,
        include_child_processes: bool = False,
    ) -> "AudioCapture":
        dll = _ffi.device.dll
        tb = _ffi.Rational(1, sample_rate)
        out = c_void_p()
        if source in ("microphone", "mic"):
            config = dll.mediaway_audio_capture_config_microphone(tb)
            _check_device(dll.mediaway_audio_capture_open(byref(config), byref(out)))
        elif source == "loopback":
            config = dll.mediaway_desktop_audio_capture_config_loopback(tb)
            _check_device(dll.mediaway_desktop_audio_capture_open(byref(config), byref(out)))
        elif source == "process_loopback":
            if process_id is None:
                raise ValueError("process_loopback requires process_id")
            config = dll.mediaway_desktop_audio_capture_config_process_loopback(
                process_id, include_child_processes, tb
            )
            _check_device(dll.mediaway_desktop_audio_capture_open(byref(config), byref(out)))
        else:
            raise ValueError(f"unknown audio source: {source!r} (microphone | loopback | process_loopback)")
        if not out.value:
            raise MediawayError(_ffi.DEVICE_UNKNOWN_ERROR, "capture open returned no handle")
        session = cls(out.value)
        session._desktop = source != "microphone"
        return session

    def sample_rate(self) -> int:
        if self._rate is None:
            self._negotiate()
        return self._rate

    def channels(self) -> int:
        if self._channels is None:
            self._negotiate()
        return self._channels

    def _negotiate(self) -> None:
        dll = _ffi.device.dll
        rate = c_uint32(0)
        channels = c_uint16(0)
        if self._desktop:
            _check_device(dll.mediaway_desktop_audio_capture_format(self._handle, byref(rate), byref(channels)))
        else:
            _check_device(dll.mediaway_audio_capture_format(self._handle, byref(rate), byref(channels)))
        self._rate = rate.value
        self._channels = channels.value

    def poll_pcm(self, timeout: float | None = None) -> bytes | None:
        """Poll the next PCM chunk; None when nothing is ready yet. Returns
        raw interleaved f32le samples (see `sample_rate()`/`channels()`)."""
        dll = _ffi.device.dll
        # The split ABI has two distinct frame struct types (identical
        # layouts) — pick the one matching this session's capture domain.
        frame_type = _ffi.DesktopAudioFrame if self._desktop else _ffi.DeviceAudioFrame
        free_fn = dll.mediaway_desktop_audio_frame_free if self._desktop else dll.mediaway_audio_frame_free
        poll_fn = (
            dll.mediaway_desktop_audio_capture_poll_frame
            if self._desktop
            else dll.mediaway_audio_capture_poll_frame
        )
        raw = frame_type()
        has = c_bool(False)
        _check_device(poll_fn(self._handle, byref(raw), byref(has)))
        if not has.value:
            return None
        try:
            return _read(raw.data, raw.data_len)
        finally:
            free_fn(byref(raw))

    def close(self) -> None:
        if self._handle:
            # Blocks up to one period interval (joins the backend worker thread).
            dll = _ffi.device.dll
            if self._desktop:
                _check_device(dll.mediaway_desktop_audio_capture_close(self._handle))
            else:
                _check_device(dll.mediaway_audio_capture_close(self._handle))
            self._handle = None

    def __enter__(self) -> "AudioCapture":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()
