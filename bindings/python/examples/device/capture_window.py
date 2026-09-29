"""Window capture quick start (Windows, WGC).

Captures one window by `HWND` through the GPU device factory (`GpuDevice`) with the options
`adr/device/0005-window-capture-c-abi.md` added: the pointer, a cropped `region`, the capture
border, and even-cropped frames that any hardware encoder accepts.

    python capture_window.py [HWND]       # decimal or 0x-prefixed; default: the foreground window

Like Screen capture there is no CPU pixel readback for these frames: `poll_frame()` proves real
frames arrive (size, pts), and real pixels only move through
`EncodeSession.write_frame_from_desktop_capture` (see `pipeline/screen_record.py`).

`region` costs nothing at the window's origin but **one GPU copy per frame** anywhere else, so it
is not Zero-Copy there. A window that later shrinks below the region raises
`RegionOutOfBoundsError`.
"""

import ctypes
import platform
import sys
import time

from mediaway import (
    CaptureUnsupportedError,
    DeviceUnavailableError,
    GpuDevice,
    MediawayError,
    Rational,
    RegionOutOfBoundsError,
    VideoCapture,
)


def pick_window() -> int:
    if len(sys.argv) > 1:
        return int(sys.argv[1], 0)
    user32 = ctypes.WinDLL("user32")
    user32.GetForegroundWindow.restype = ctypes.c_void_p
    return user32.GetForegroundWindow() or 0


def main() -> None:
    if platform.system() != "Windows":
        print("Window capture is Windows-only (an HWND source); skipping.")
        return
    hwnd = pick_window()
    if not hwnd:
        print("no window: pass an HWND, or focus a window first")
        return

    try:
        with GpuDevice.create(video_support=True) as gpu:
            capture = VideoCapture.open(
                source="window",
                window=hwnd,
                gpu_device=gpu,
                frame_rate=Rational(1, 30),
                cursor="included",  # draw the pointer where it overlaps the window
                border="hidden",  # Windows 11 build 22000+; a refusal is not an error...
                dimensions="even_cropped",  # ...odd sizes lose one pixel so any encoder accepts them
            )
            with capture:
                # ...so ask what the OS actually did.
                print(f"border hidden: {capture.border_hidden}")
                width, height = capture.size()
                print(f"window geometry: {width}x{height}")

                deadline = time.monotonic() + 3.0
                count = 0
                while time.monotonic() < deadline and count < 5:
                    frame = capture.poll_frame()
                    if frame is not None:
                        print(f"  frame {count + 1}: {frame.width}x{frame.height} fmt={frame.format.name}")
                        capture.release_frame()
                        count += 1
                    else:
                        time.sleep(0.01)
                print(f"captured {count} real frame(s)")
                if count == 0:
                    print("(WGC delivers on change: a still window produces no new frames)")
    except RegionOutOfBoundsError as err:
        print(f"region does not fit the window: {err}")
    except (CaptureUnsupportedError, DeviceUnavailableError) as err:
        print(f"Window capture unavailable on this machine: {err}")
    except MediawayError as err:
        print(f"Window capture failed (status {err.status}): {err}")


if __name__ == "__main__":
    main()
