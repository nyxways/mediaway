# C++: GPU device factory + real Screen capture + capture-encode bridge

`bindings/cpp/include/mediaway/{device,pipeline}.hpp` — the last of the four
planned Tier B bindings to close this gap (see
[nodejs-gpu-device](nodejs-gpu-device.md), [csharp-gpu-device](csharp-gpu-device.md),
[python-gpu-device](python-gpu-device.md), [language-bindings](language-bindings.md)):
before this, `device::ScreenCapture::open()` unconditionally threw
`Error(Status::Unsupported)` — no C++ caller had any way to construct a GPU
device, and Screen capture has no CPU fallback. `ScreenCapture::pollFrame()`
also unconditionally threw on a GPU-storage frame — dead code, since Screen
could never actually open to reach it.

## `device::GpuDevice` (new, in `device.hpp`)

`GpuDevice::listAdapters()` (static) and `GpuDevice::create(GpuDeviceOptions)`
(static) — `GpuDeviceOptions::adapterIndex` is `std::optional<uint32_t>`
(`nullopt` = backend default adapter). `.handle()` returns the
`mediaway_gpu_device_handle_t` value both `ScreenCaptureConfig::gpuDevice` and
`encoder::VideoEncoderConfig::gpuDevice` now accept. `listAdapters()` is this
binding's first "owned `T**` array + bulk free" wrapper — no existing
container wrapper used that idiom (`Demuxer::streams()` is count-then-index
with a per-item free instead); the loop that converts each entry runs inside a
`try`/`catch (...) { mediaway_gpu_adapter_list_free(...); throw; }` so the
array is freed even if a mid-loop `std::string` allocation throws
`std::bad_alloc`. RAII matches every other class here: `unique_ptr` + a
function-pointer deleter (`mediaway_gpu_device_close`), move-only.

## `ScreenCapture` changes

`ScreenCaptureConfig` gained a mandatory `gpuDevice` field; `open()` now
calls the real `mediaway_desktop_capture_config_screen`/`_open` instead of
throwing, and gained a `queryGeometry()` step `VideoCapture` already had but
`ScreenCapture` never did (unreachable before — `info()` stayed `{0,0,...}`
forever). `pollFrame()` no longer throws on `MEDIAWAY_VIDEO_FRAME_STORAGE_GPU`
— `VideoFrame::data` stays empty for that case (no CPU pixel readback path in
the wrapped Rust backend), proving a real frame arrived via width/height/pts
without fabricating pixel data. Gained `releaseFrame()` (previously missing
entirely) — the real release point for a GPU frame's texture, caller-driven
like every other binding's Screen capture, unlike Camera's documented no-op.

## Capture-to-encode bridge

`EncodeSession::writeFrameFromCameraCapture(device::VideoCapture&)` /
`.writeFrameFromDesktopCapture(device::ScreenCapture&)` — poll-and-push in one
native call, no intermediate `VideoFrame`, no CPU copy for Screen's GPU frames
(`adr/pipeline/0005-capture-encode-bridge-c-abi.md`). Reaching each capture
class's opaque handle from `EncodeSession` (a different class, same `mediaway`
namespace tree) needed a real access-control decision — C++'s answer is
`friend class encoder::EncodeSession;` inside `VideoCapture`/`ScreenCapture`,
plus a private `rawHandle()` accessor each only grants to that one friend, and
a forward declaration (`namespace encoder { class EncodeSession; }`) added to
`device.hpp` since `pipeline.hpp` includes `device.hpp`, not the reverse.
Conceptually the same problem C# solved with `InternalsVisibleTo` (assembly
granularity) and Python didn't need to solve at all (no real access control)
— C++'s `friend` is the language-native equivalent at class granularity, and
was already the right tool sitting there unused.

## Verified

Real hardware, this session: `GpuDevice::listAdapters()` enumerated the real
RTX 4090 + iGPU + WARP adapters; `ScreenCapture::open()` captured 5 real
2560x1440 GPU-backed frames (`examples/device/capture_screen.cpp`, rewritten
from a permanent-gap demo to a real, `-Wall`-clean link+run); the bridge
reached a real `AutoVideoEncoder::open` with `gpuDevice` set and
`writeFrameFromDesktopCapture`, gracefully hitting the same known dev-machine
WMF/DX11 GPU-input-encode limitation the Rust/C/Node.js/C#/Python siblings hit
(`examples/pipeline/screen_record.cpp`, same rewrite — its
`AutoVideoEncoder`/`optional::emplace` early-catch mirrors the existing
`mic`/`AudioCapture` pattern already in the file, since `AutoVideoEncoder` has
no default constructor to support a bare declare-then-assign). All 7
pre-existing examples re-compiled and re-ran clean (no regression), including
real camera/mic/two-track-MP4 (`camera_record.cpp`).

## A stale doc claim fixed while touching `screen_record.cpp`

Same issue as Python's `screen_record.py` before this: its header claimed "no
audio encoder exists in the ABI", false since `AudioEncoder` (ABI v2) already
shipped and `camera_record.cpp`'s own header already says that gap is closed.
Corrected the wording; did not add this file's own two-track remux (out of
scope for this GPU-device pass, matching every other binding's
`screen_record` — mic PCM stays drained, not muxed).
