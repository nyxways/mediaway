# Device

Four capture sources share two traits — `VideoCapture` (screen, window,
camera) and `AudioCapture` (microphone) — both a poll loop over frames, no
encoding involved. What differs per source is how much of it is wired into
`mediaway_pipeline::platform`'s cross-platform dispatch versus needing a
platform-specific type directly.

## Screen — fully dispatched

```rust,ignore
let config = VideoCaptureConfig::screen(Select::Default, Rational::new(1, 30));
let mut capture = platform::ScreenCapture::open(&config)?;

while let Some(frame) = capture.poll_frame()? {
    // consume frame …
    capture.release_frame()?;
}
capture.close()?;
```

`release_frame` matters for GPU-backed sources: it frees the backend's hold
on that frame (e.g. DXGI's `ReleaseFrame`) before the next `poll_frame` can
acquire again.

Try it: `cargo run --example capture_screen` —
[`examples/device/capture_screen.rs`](https://github.com/nyxways/mediaway/blob/main/examples/device/capture_screen.rs).

## Microphone — fully dispatched

```rust,ignore
let config = AudioCaptureConfig::microphone(Rational::new(1, 48_000));
let mut mic = platform::Microphone::open(&config)?;

while let Some(frame) = mic.poll_frame()? {
    // frame.data is interleaved PCM
}
```

Try it: `cargo run --example capture_microphone` —
[`examples/device/capture_microphone.rs`](https://github.com/nyxways/mediaway/blob/main/examples/device/capture_microphone.rs).

## Camera — platform type directly

Camera capture isn't wired into `platform` yet, so reach for the Windows
backend directly — it compiles on every platform (a stub returns
`CaptureError::Unsupported` off Windows, same failure shape as a missing
camera on Windows itself):

```rust,ignore
let config = VideoCaptureConfig {
    source: CaptureSource::Camera { select: Select::Default },
    time_base: Rational::new(1, 30),
    // Media Foundation's camera backend is CPU-frames-only today.
    output: CaptureOutputPreference::CpuFramesOk,
    gpu_device: None,
};
let mut camera = mediaway_device_windows::WindowsCameraCapture::open(&config)?;
```

Try it: `cargo run --example capture_camera` —
[`examples/device/capture_camera.rs`](https://github.com/nyxways/mediaway/blob/main/examples/device/capture_camera.rs).

## Window — needs a caller-owned GPU device

Window capture (`WinRT` Graphics Capture) is the one source with **no
CPU-only path** — `open()` requires both a live `HWND` and a caller-owned
`ID3D11Device` handed in as `gpu_device: Some(GpuDeviceHandle::DirectX11(handle))`.
Obtaining either means calling raw Win32/WinRT FFI, which is `unsafe` — out
of scope for a plain example, so
[`examples/device/capture_window.rs`](https://github.com/nyxways/mediaway/blob/main/examples/device/capture_window.rs)
only shows the config shape. For a complete, hardware-tested version with
the `unsafe` fully contained and documented, see
`crates/mediaway-device-windows/src/lib_tests.rs`'s
`open_window_capture_foreground_or_skip` test in the repository.

## What's available where

See [Device](../reference/device-capture.md) for the full
platform/source support matrix.
