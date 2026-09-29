"""Window capture + capture options over the C ABI (adr/device/0005-window-capture-c-abi.md).

Assert-based script, no pytest dependency — same contract as the other tests here:

    python tests/test_window_capture.py

Everything except the last section is deterministic and needs no window: the struct layout, the
zero-means-previous-behaviour defaults, the status mapping, and the configs the ABI must refuse
before any capture starts.

The real-hardware section shows a real window on the desktop, so it is opt-in:

    MEDIAWAY_RUN_WINDOW_TESTS=1 python tests/test_window_capture.py

It needs Windows 11 (borderless capture) and a GPU device.
"""

import ctypes
import os
import platform
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from mediaway import (  # noqa: E402
    CaptureUnsupportedError,
    DeviceUnavailableError,
    GpuDevice,
    MediawayError,
    Rational,
    RegionOutOfBoundsError,
    VideoCapture,
    _ffi,
)
from mediaway._device import _check_device  # noqa: E402

IS_WINDOWS = platform.system() == "Windows"
TB = Rational(1, 30)


def test_layout_matches_the_header() -> None:
    # Authoritative numbers: `offsetof` from compiling crates/mediaway-ffi/include/mediaway/device.h
    # with gcc (2026-09-29). A reordered or resized field here corrupts every config passed by value.
    expected = {
        "source_kind": 0,
        "source_index": 4,
        "time_base": 8,
        "gpu_device": 24,
        "window_handle": 48,
        "cursor": 56,
        "border": 60,
        "dimensions": 64,
        "region_x": 68,
        "region_y": 72,
        "region_width": 76,
        "region_height": 80,
        "region_enabled": 84,
    }
    struct = _ffi.DesktopCaptureConfig
    assert ctypes.sizeof(struct) == 88, ctypes.sizeof(struct)
    for name, offset in expected.items():
        assert getattr(struct, name).offset == offset, f"{name} at {getattr(struct, name).offset}, want {offset}"


def test_abi_version_is_two() -> None:
    # The struct grew, so the device ABI is 2; a 1 here means a stale native library.
    assert _ffi.device.dll.mediaway_device_ffi_abi_version() == 2


def _none_gpu() -> "_ffi.GpuDeviceHandle":
    return _ffi.GpuDeviceHandle(kind=_ffi.GPU_DEVICE_NONE, native=0, webgpu_device_id=0)


def test_new_fields_default_to_previous_behaviour() -> None:
    dll = _ffi.device.dll
    tb = _ffi.Rational(TB.num, TB.den)
    for config in (
        dll.mediaway_desktop_capture_config_screen(0, tb, _none_gpu()),
        dll.mediaway_desktop_capture_config_window(0xABCD, tb, _none_gpu()),
    ):
        assert (config.cursor, config.border, config.dimensions) == (0, 0, 0)
        assert not config.region_enabled
        assert (config.region_x, config.region_y, config.region_width, config.region_height) == (0, 0, 0, 0)
    window = dll.mediaway_desktop_capture_config_window(0x1_0000_0001, tb, _none_gpu())
    assert window.source_kind == _ffi.DESKTOP_SOURCE_WINDOW
    assert window.window_handle == 0x1_0000_0001  # a full 64-bit value survives ctypes


def test_status_14_is_its_own_exception() -> None:
    assert _ffi.DEVICE_REGION_OUT_OF_BOUNDS == 14
    try:
        _check_device(14)
    except RegionOutOfBoundsError as err:
        assert err.status == 14
        assert isinstance(err, MediawayError)
    else:
        raise AssertionError("status 14 did not raise RegionOutOfBoundsError")
    # Invalid input stays a plain MediawayError: not retryable the way a shrunken window is.
    try:
        _check_device(_ffi.DEVICE_INVALID_INPUT)
    except RegionOutOfBoundsError:
        raise AssertionError("INVALID_INPUT must not map to RegionOutOfBoundsError") from None
    except MediawayError as err:
        assert err.status == _ffi.DEVICE_INVALID_INPUT


def test_bad_option_strings_fail_before_any_device_is_created() -> None:
    for kwargs in ({"cursor": "yes"}, {"border": "none"}, {"dimensions": "square"}):
        try:
            VideoCapture.open(source="window", window=1, **kwargs)
        except ValueError:
            pass
        else:
            raise AssertionError(f"{kwargs} should be rejected with ValueError")
    try:
        VideoCapture.open(source="window")
    except ValueError:
        pass
    else:
        raise AssertionError("source='window' without window= should be a ValueError")


def _expect_status(expected_exception, expected_status, **open_kwargs) -> None:
    try:
        session = VideoCapture.open(**open_kwargs)
    except expected_exception as err:
        if expected_status is not None:
            assert err.status == expected_status, (err.status, expected_status)
    except DeviceUnavailableError:
        print(f"  skipped {open_kwargs}: no GPU device / backend on this machine")
    else:
        session.close()
        raise AssertionError(f"{open_kwargs} unexpectedly opened")


def test_refused_configs() -> None:
    if not IS_WINDOWS:
        # No HWND source off Windows: the ABI answers UNSUPPORTED for every Window config.
        _expect_status(CaptureUnsupportedError, _ffi.DEVICE_UNSUPPORTED, source="window", window=1)
        return
    # An HWND of 0 names no window.
    _expect_status(MediawayError, _ffi.DEVICE_INVALID_INPUT, source="window", window=0)
    # An enabled region with a zero side is never valid.
    _expect_status(MediawayError, _ffi.DEVICE_INVALID_INPUT, source="screen", region=(0, 0, 0, 64))
    # `border` / `dimensions` are Window-only: refused on Screen, not dropped silently.
    _expect_status(MediawayError, _ffi.DEVICE_INVALID_INPUT, source="screen", border="hidden")
    _expect_status(MediawayError, _ffi.DEVICE_INVALID_INPUT, source="screen", dimensions="even_cropped")
    # DXGI cannot draw the pointer or crop: UNSUPPORTED, not ignored.
    _expect_status(CaptureUnsupportedError, _ffi.DEVICE_UNSUPPORTED, source="screen", cursor="included")
    _expect_status(CaptureUnsupportedError, _ffi.DEVICE_UNSUPPORTED, source="screen", region=(0, 0, 64, 64))


# ── real hardware (opt-in) ───────────────────────────────────────────────────


def _create_test_window(width: int, height: int):
    """A real visible top-level window via user32: WGC cannot capture a missing or minimized one."""
    from ctypes import wintypes

    user32 = ctypes.WinDLL("user32", use_last_error=True)
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.GetModuleHandleW.restype = wintypes.HMODULE
    kernel32.GetModuleHandleW.argtypes = [wintypes.LPCWSTR]
    user32.DefWindowProcW.restype = ctypes.c_ssize_t
    user32.DefWindowProcW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]

    wndproc_t = ctypes.WINFUNCTYPE(ctypes.c_ssize_t, wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM)

    class WNDCLASSW(ctypes.Structure):
        _fields_ = [
            ("style", wintypes.UINT),
            ("lpfnWndProc", wndproc_t),
            ("cbClsExtra", ctypes.c_int),
            ("cbWndExtra", ctypes.c_int),
            ("hInstance", wintypes.HINSTANCE),
            ("hIcon", wintypes.HANDLE),
            ("hCursor", wintypes.HANDLE),
            ("hbrBackground", wintypes.HANDLE),
            ("lpszMenuName", wintypes.LPCWSTR),
            ("lpszClassName", wintypes.LPCWSTR),
        ]

    user32.RegisterClassW.argtypes = [ctypes.POINTER(WNDCLASSW)]
    user32.RegisterClassW.restype = wintypes.ATOM
    user32.CreateWindowExW.restype = wintypes.HWND
    user32.CreateWindowExW.argtypes = [
        wintypes.DWORD, wintypes.LPCWSTR, wintypes.LPCWSTR, wintypes.DWORD,
        ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
        wintypes.HWND, wintypes.HMENU, wintypes.HINSTANCE, wintypes.LPVOID,
    ]  # fmt: skip
    user32.ShowWindow.argtypes = [wintypes.HWND, ctypes.c_int]
    user32.DestroyWindow.argtypes = [wintypes.HWND]
    user32.UnregisterClassW.argtypes = [wintypes.LPCWSTR, wintypes.HINSTANCE]

    instance = kernel32.GetModuleHandleW(None)
    class_name = "MediawayPyWindowCaptureTest"
    proc = wndproc_t(("DefWindowProcW", user32))  # keep a reference alive for the window's lifetime
    wc = WNDCLASSW(lpfnWndProc=proc, hInstance=instance, lpszClassName=class_name)
    if not user32.RegisterClassW(ctypes.byref(wc)):
        raise OSError(f"RegisterClassW failed ({ctypes.get_last_error()})")
    WS_OVERLAPPEDWINDOW, CW_USEDEFAULT, SW_SHOWNORMAL = 0x00CF0000, 0x80000000 - 2**32, 1
    hwnd = user32.CreateWindowExW(
        0, class_name, "mediaway python window capture", WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT, CW_USEDEFAULT, width, height, None, None, instance, None,
    )  # fmt: skip
    if not hwnd:
        raise OSError(f"CreateWindowExW failed ({ctypes.get_last_error()})")
    user32.ShowWindow(hwnd, SW_SHOWNORMAL)

    def destroy() -> None:
        user32.DestroyWindow(hwnd)
        user32.UnregisterClassW(class_name, instance)

    return hwnd, destroy, proc


def _next_frame(session: VideoCapture, seconds: float = 3.0):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        frame = session.poll_frame()
        if frame is not None:
            session.release_frame()
            return frame
        time.sleep(0.01)
    raise AssertionError(f"no frame within {seconds} s")


def test_real_window_capture() -> None:
    # 335x250 makes WGC's capture size odd on both axes at 100% scale on Windows 11 (it leaves out
    # the invisible resize borders), so even-cropping is observable. Checked, not trusted.
    hwnd, destroy, _proc = _create_test_window(335, 250)
    try:
        with GpuDevice.create(video_support=True) as gpu:
            with VideoCapture.open(source="window", window=hwnd, gpu_device=gpu) as plain:
                native = plain.size()
                assert native[0] % 2 == 1 or native[1] % 2 == 1, (
                    f"the test window captured at {native}, even on both axes, so this proves nothing about cropping"
                )
                assert plain.border_hidden is False, "border='shown' must report not hidden"
                first = _next_frame(plain)  # once: WGC delivers on change, a still window gives no second frame
                assert (first.width, first.height) == native

            with VideoCapture.open(
                source="window", window=hwnd, gpu_device=gpu,
                cursor="included", border="hidden", dimensions="even_cropped",
            ) as cropped:  # fmt: skip
                size = cropped.size()
                assert size == (native[0] & ~1, native[1] & ~1), (size, native)
                assert size[0] % 2 == 0 and size[1] % 2 == 0
                # Asserted, not skipped: a skip is how the border once went unhidden unnoticed.
                assert cropped.border_hidden is True, "the OS refused to hide the border (needs Windows 11 22000+)"
                frame = _next_frame(cropped)
                assert (frame.width, frame.height) == size

            # A region at the origin is free (the pool is sized to it); one away from it is one GPU copy
            # per frame. Both deliver frames of exactly the region's size.
            for x, y in ((0, 0), (10, 8)):
                with VideoCapture.open(source="window", window=hwnd, gpu_device=gpu, region=(x, y, 64, 48)) as region:
                    assert region.size() == (64, 48)
                    frame = _next_frame(region)
                    assert (frame.width, frame.height) == (64, 48), (x, y, frame.width, frame.height)

            # Larger than the window: its own exception, not INVALID_INPUT.
            try:
                VideoCapture.open(
                    source="window", window=hwnd, gpu_device=gpu, region=(0, 0, native[0] + 100, native[1] + 100)
                )
            except RegionOutOfBoundsError as err:
                assert err.status == _ffi.DEVICE_REGION_OUT_OF_BOUNDS
            else:
                raise AssertionError("an oversized region opened")
        print(f"  real window: native {native}, cropped {size}, regions ok")
    finally:
        destroy()


def main() -> None:
    tests = [
        test_layout_matches_the_header,
        test_abi_version_is_two,
        test_new_fields_default_to_previous_behaviour,
        test_status_14_is_its_own_exception,
        test_bad_option_strings_fail_before_any_device_is_created,
        test_refused_configs,
    ]
    if os.environ.get("MEDIAWAY_RUN_WINDOW_TESTS") == "1":
        if not IS_WINDOWS:
            print("SKIP real window test: Windows only")
        else:
            tests.append(test_real_window_capture)
    else:
        print("SKIP real window test: set MEDIAWAY_RUN_WINDOW_TESTS=1 (it shows a window)")
    for test in tests:
        test()
        print(f"ok  {test.__name__}")
    print(f"PASS: {len(tests)} window capture tests")


if __name__ == "__main__":
    main()
