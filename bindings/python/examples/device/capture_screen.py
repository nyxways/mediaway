"""Screen capture quick start.

Real Zero-Copy Screen capture (DXGI Desktop Duplication) via the GPU device
factory (`GpuDevice`, adr/0007-gpu-device-factory.md) — closes the "no Python
caller can construct a GPU device" gap: `VideoCapture.open(source="screen")`
creates one internally here (or pass your own via the `gpu_device` kwarg to
share it with an encoder). There is no CPU pixel readback path for Screen in
the wrapped Rust backend — `poll_frame()` proves frames are genuinely arriving
(real pts/geometry) but `VideoFrame.data` is always empty; real pixels only
ever move through `EncodeSession.write_frame_from_desktop_capture` (see
`pipeline/screen_record.py`).
"""

import time

from mediaway import CaptureUnsupportedError, DeviceUnavailableError, Rational, VideoCapture


def main() -> None:
    try:
        capture = VideoCapture.open(source="screen", index=0, frame_rate=Rational(1, 30))
    except (CaptureUnsupportedError, DeviceUnavailableError) as err:
        print(f"Screen capture unavailable on this machine: {err}")
        return

    with capture:
        width, height = capture.size()
        print(f"Screen geometry: {width}x{height}")

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


if __name__ == "__main__":
    main()
