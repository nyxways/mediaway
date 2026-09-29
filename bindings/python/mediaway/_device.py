"""Device capability: camera / screen / microphone capture + the GPU device factory.

Wraps the `mediaway-ffi` C ABI (see `_ffi.py` for the raw layer and
`../README.md` for the DX contract). The ABI is domain-split
(`adr/0004-domain-feature-split.md`): Camera (`mediaway_camera_capture_*`),
Desktop/Screen (`mediaway_desktop_capture_*`), Microphone
(`mediaway_audio_capture_*`), and Desktop audio / loopback
(`mediaway_desktop_audio_capture_*`). This module's public API folds the split
back into the two DX classes, `VideoCapture` and `AudioCapture`.

Screen capture needs a live GPU device handle (`ID3D11Device*`) with no CPU
fallback — `GpuDevice` (adr/0007-gpu-device-factory.md) is the factory that
builds one; `VideoCapture.open(source="screen")` creates one internally when
the caller doesn't supply their own. Camera delivers owned CPU frames; Screen
delivers a GPU-backed texture handle with no pixel readback path in the
wrapped Rust backend — `poll_frame()` proves frames are genuinely arriving
(real pts/geometry) but `VideoFrame.data` is always empty for Screen; real
pixels only ever move through `EncodeSession.write_frame_from_desktop_capture`
(adr/pipeline/0005-capture-encode-bridge-c-abi.md). Audio capture delivers raw
interleaved PCM (`poll_pcm`), not encoded audio — there is no audio encoder in
this module (see `_encoder.py`'s `AudioEncoder`).
"""

from __future__ import annotations

from ctypes import POINTER, byref, c_bool, c_size_t, c_uint16, c_uint32, c_void_p

from . import _ffi
from ._container import _from_units
from ._errors import CaptureUnsupportedError, DeviceUnavailableError, MediawayError, RegionOutOfBoundsError
from ._types import GpuAdapter, PixelFormat, Rational, VideoFrame

__all__ = ["VideoCapture", "AudioCapture", "GpuDevice"]


def _check_device(status: int) -> None:
    if status == _ffi.DEVICE_OK:
        return
    if status in (_ffi.DEVICE_NO_BACKEND, _ffi.DEVICE_BACKEND_FAILURE, _ffi.DEVICE_ACCESS_DENIED):
        raise DeviceUnavailableError(status, "no capture backend or device available")
    if status == _ffi.DEVICE_UNSUPPORTED:
        raise CaptureUnsupportedError(status, "this capture configuration is unsupported by the ABI")
    if status == _ffi.DEVICE_REGION_OUT_OF_BOUNDS:
        raise RegionOutOfBoundsError(status, "the capture region does not fit the captured surface")
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


_CURSORS = {"excluded": _ffi.CAPTURE_CURSOR_EXCLUDED, "included": _ffi.CAPTURE_CURSOR_INCLUDED}
_BORDERS = {"shown": _ffi.CAPTURE_BORDER_SHOWN, "hidden": _ffi.CAPTURE_BORDER_HIDDEN}
_DIMENSIONS = {"native": _ffi.FRAME_DIMENSIONS_NATIVE, "even_cropped": _ffi.FRAME_DIMENSIONS_EVEN_CROPPED}


def _choice(table: dict[str, int], name: str, value: str) -> int:
    try:
        return table[value]
    except KeyError:
        raise ValueError(f"unknown {name}: {value!r} ({' | '.join(table)})") from None


def _apply_options(
    config: "_ffi.DesktopCaptureConfig",
    cursor: str,
    border: str,
    dimensions: str,
    region: tuple[int, int, int, int] | None,
) -> None:
    """Write the ADR-0005 option fields into `config`. The defaults are the zero values, i.e.
    exactly what a config built before those fields existed captured."""
    config.cursor = _choice(_CURSORS, "cursor", cursor)
    config.border = _choice(_BORDERS, "border", border)
    config.dimensions = _choice(_DIMENSIONS, "dimensions", dimensions)
    if region is not None:
        x, y, width, height = region
        config.region_enabled = True
        config.region_x, config.region_y = x, y
        config.region_width, config.region_height = width, height


def _read(ptr, length: int) -> bytes:
    import ctypes

    if not ptr or length == 0:
        return b""
    return ctypes.string_at(ptr, length)


class GpuDevice:
    """A real GPU device (e.g. a DirectX11 `ID3D11Device`) created by the
    native backend — closes the "no Python caller can construct a GPU device"
    gap for Screen capture (`VideoCapture.open(source="screen")`) and
    GPU-input encode (`AutoVideoEncoder.pick(..., gpu_device=...)`), both of
    which require a live device handle with no CPU fallback.
    """

    def __init__(self, handle: int, gpu_handle: "_ffi.GpuDeviceHandle"):
        self._handle = handle
        self.handle = gpu_handle  # _ffi.GpuDeviceHandle — pass into other opens/configs

    @staticmethod
    def list_adapters() -> list[GpuAdapter]:
        """Enumerate every DXGI adapter on this machine (name, VRAM, hardware-vs-software)."""
        dll = _ffi.device.dll
        adapters_ptr = POINTER(_ffi.GpuAdapterInfo)()
        count = c_size_t(0)
        _check_device(dll.mediaway_gpu_adapter_list(byref(adapters_ptr), byref(count)))
        try:
            result = []
            for i in range(count.value):
                entry = adapters_ptr[i]
                result.append(
                    GpuAdapter(
                        index=entry.index,
                        name=(entry.name or b"").decode("utf-8", errors="replace"),
                        vendor_id=entry.vendor_id,
                        device_id=entry.device_id,
                        dedicated_video_memory=entry.dedicated_video_memory,
                        is_hardware=bool(entry.is_hardware),
                    )
                )
            return result
        finally:
            dll.mediaway_gpu_adapter_list_free(adapters_ptr, count)

    @classmethod
    def create(
        cls,
        *,
        adapter_index: int | None = None,
        video_support: bool = False,
        debug_layer: bool = False,
    ) -> "GpuDevice":
        """Build a real device. `adapter_index=None` (default) picks the
        backend's default adapter (first hardware adapter on Windows); pass an
        index from `list_adapters()` to select one explicitly. `video_support`
        is required for GPU-input encode."""
        dll = _ffi.device.dll
        if adapter_index is None:
            adapter = _ffi.GpuAdapterSelect(kind=_ffi.GPU_ADAPTER_SELECT_DEFAULT, index=0)
        else:
            adapter = _ffi.GpuAdapterSelect(kind=_ffi.GPU_ADAPTER_SELECT_INDEX, index=adapter_index)
        options = _ffi.GpuDeviceOptions(adapter=adapter, video_support=video_support, debug_layer=debug_layer)
        out = c_void_p()
        _check_device(dll.mediaway_gpu_device_create(byref(options), byref(out)))
        if not out.value:
            raise MediawayError(_ffi.DEVICE_UNKNOWN_ERROR, "GPU device create returned no handle")
        gpu_handle = _ffi.GpuDeviceHandle()
        status = dll.mediaway_gpu_device_handle(out.value, byref(gpu_handle))
        if status != _ffi.DEVICE_OK:
            dll.mediaway_gpu_device_close(out.value)
            _check_device(status)
        return cls(out.value, gpu_handle)

    def close(self) -> None:
        """Releases the native device. Every handle obtained from it becomes invalid immediately."""
        if self._handle:
            _ffi.device.dll.mediaway_gpu_device_close(self._handle)
            self._handle = None

    def __enter__(self) -> "GpuDevice":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()


class VideoCapture:
    """A video capture session. `source="camera"` opens the Camera ABI (real,
    CPU frames); `source="screen"` opens real Zero-Copy Screen capture (DXGI
    Desktop Duplication) — a `GpuDevice` is created internally when
    `gpu_device` is omitted (and closed on `close()`); a caller-supplied
    device is left open (caller owns it), letting one device be shared
    between capture and `AutoVideoEncoder.pick(gpu_device=...)`.
    `source="window"` captures one window by `HWND` (WGC, Windows only; other
    platforms raise `CaptureUnsupportedError`) and needs a GPU device the same
    way Screen does.

    Desktop options (Screen and Window; `adr/device/0005-window-capture-c-abi.md`):

    - `cursor`: `"excluded"` (default) or `"included"`. DXGI Screen capture cannot
      draw the pointer, so `"included"` there raises `CaptureUnsupportedError`.
    - `region`: `(x, y, width, height)` to record only that rectangle, or `None`.
      WGC only. **A region away from the window's origin costs one GPU copy per
      frame** (not Zero-Copy). DXGI Screen capture raises
      `CaptureUnsupportedError`; a window smaller than the region raises
      `RegionOutOfBoundsError`.
    - `border` (`"shown"` default / `"hidden"`) and `dimensions` (`"native"`
      default / `"even_cropped"`) are Window-only; passing a non-default value
      with `source="screen"` is an invalid-input error, never ignored.
      `"hidden"` needs Windows 11 build 22000+; check `border_hidden` for what
      the OS actually did. `"even_cropped"` trims an odd axis by one pixel so
      any hardware encoder accepts the frames, at no cost."""

    def __init__(self, handle: int, time_base: Rational, source: str = "camera"):
        self._handle = handle
        self._time_base = time_base
        self._source = source
        self._owned_gpu_device: GpuDevice | None = None

    @classmethod
    def open(
        cls,
        source: str = "camera",
        index: int = 0,
        frame_rate: Rational = Rational(1, 30),
        gpu_device: GpuDevice | None = None,
        *,
        window: int | None = None,
        cursor: str = "excluded",
        border: str = "shown",
        dimensions: str = "native",
        region: tuple[int, int, int, int] | None = None,
    ) -> "VideoCapture":
        """Open a capture session. `index` is the camera or display-output ordinal;
        `window` is the caller-owned `HWND` for `source="window"` (`0` is rejected
        as invalid input by the ABI)."""
        dll = _ffi.device.dll
        tb = _ffi.Rational(frame_rate.num, frame_rate.den)
        out = c_void_p()
        if source == "camera":
            config = dll.mediaway_camera_capture_config_default(index, tb)
            _check_device(dll.mediaway_camera_capture_open(byref(config), byref(out)))
            return cls(out.value, frame_rate, source="camera")
        elif source in ("screen", "window"):
            if source == "window" and window is None:
                raise ValueError('source="window" requires window=<HWND>')
            # Validate the option strings before creating a GPU device we would then have to close.
            _apply_options(_ffi.DesktopCaptureConfig(), cursor, border, dimensions, region)
            owned_gpu_device = None
            if gpu_device is None:
                gpu_device = owned_gpu_device = GpuDevice.create(video_support=True)
            if source == "screen":
                config = dll.mediaway_desktop_capture_config_screen(index, tb, gpu_device.handle)
            else:
                config = dll.mediaway_desktop_capture_config_window(window, tb, gpu_device.handle)
            _apply_options(config, cursor, border, dimensions, region)
            status = dll.mediaway_desktop_capture_open(byref(config), byref(out))
            if status != _ffi.DEVICE_OK:
                if owned_gpu_device is not None:
                    owned_gpu_device.close()
                _check_device(status)
            session = cls(out.value, frame_rate, source=source)
            session._owned_gpu_device = owned_gpu_device
            return session
        else:
            raise ValueError(f"unknown video source: {source!r} (camera | screen | window)")

    def size(self) -> tuple[int, int]:
        """The negotiated frame width/height (do not assume a resolution)."""
        dll = _ffi.device.dll
        w = c_uint32(0)
        h = c_uint32(0)
        if self._source == "camera":
            _check_device(dll.mediaway_camera_capture_geometry(self._handle, byref(w), byref(h)))
        else:
            _check_device(dll.mediaway_desktop_capture_geometry(self._handle, byref(w), byref(h)))
        return w.value, h.value

    def frame_rate(self) -> Rational:
        """The configured frame period (the ABI does not re-negotiate it)."""
        return self._time_base

    @property
    def border_hidden(self) -> bool:
        """Whether the OS actually hid the capture border of a Window session.

        A refused `border="hidden"` is not an error at open (the border is drawn
        on screen, never into frames), so this is the only way to learn it.
        Raises `CaptureUnsupportedError` for Camera and Screen sessions."""
        if self._source != "window":
            raise CaptureUnsupportedError(_ffi.DEVICE_UNSUPPORTED, "only a Window session has a capture border")
        hidden = c_bool(False)
        _check_device(_ffi.device.dll.mediaway_desktop_capture_border_hidden(self._handle, byref(hidden)))
        return hidden.value

    def poll_frame(self, timeout: float | None = None) -> VideoFrame | None:
        """Poll the next frame; None when nothing is ready yet. With a
        `timeout` (seconds), blocks until a frame or the deadline — Camera
        only; Screen/Window always poll non-blocking regardless of `timeout`.

        For Screen, `data` is always empty: there is no CPU pixel readback
        path for GPU-backed frames in the wrapped Rust backend (this call
        still proves real frames are arriving via `pts`/geometry). Real
        pixels only ever move through
        `EncodeSession.write_frame_from_desktop_capture`."""
        dll = _ffi.device.dll
        if self._source == "camera":
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

        raw = _ffi.DesktopFrame()
        has = c_bool(False)
        _check_device(dll.mediaway_desktop_capture_poll_frame(self._handle, byref(raw), byref(has)))
        if not has.value:
            return None
        try:
            data = _read(raw.data, raw.data_len) if raw.storage_kind == _ffi.STORAGE_CPU else b""
            return VideoFrame(
                width=raw.width,
                height=raw.height,
                format=PixelFormat(raw.pixel_format),
                data=data,
                pts=_from_units(raw.pts, self._time_base),
                duration=_from_units(raw.duration, self._time_base) if raw.duration else None,
            )
        finally:
            # Documented no-op for the GPU case (nothing owned by this struct
            # then) — always safe to call, same as the CPU case.
            dll.mediaway_desktop_frame_free(byref(raw))

    def release_frame(self) -> None:
        """Release backend resources held by the last polled frame. Documented
        no-op for Camera today, but still required before the next poll.
        For Screen, this is the real release point for the polled frame's GPU
        texture — call it before the next acquiring poll."""
        dll = _ffi.device.dll
        if self._source == "camera":
            _check_device(dll.mediaway_camera_capture_release_frame(self._handle))
        else:
            _check_device(dll.mediaway_desktop_capture_release_frame(self._handle))

    def close(self) -> None:
        if self._handle:
            dll = _ffi.device.dll
            # Blocks up to one frame interval (joins the backend worker thread).
            if self._source == "camera":
                _check_device(dll.mediaway_camera_capture_close(self._handle))
            else:
                _check_device(dll.mediaway_desktop_capture_close(self._handle))
            self._handle = None
        if self._owned_gpu_device is not None:
            self._owned_gpu_device.close()
            self._owned_gpu_device = None

    def __enter__(self) -> "VideoCapture":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()


class AudioCapture:
    """An audio capture session. `source="microphone"` opens the Microphone
    ABI; `"loopback"`/`"process_loopback"` open the Desktop-audio ABI (capture
    of what the desktop is rendering).

    For `"process_loopback"`, `include_target_process_tree=True` captures
    `process_id` and its descendants; `False` captures everything the desktop
    renders *except* that process tree. Windows has no "target process alone"
    mode, so `False` is the complement of `True`, not a narrowing of it."""

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
        include_target_process_tree: bool = False,
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
                process_id, include_target_process_tree, tb
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
