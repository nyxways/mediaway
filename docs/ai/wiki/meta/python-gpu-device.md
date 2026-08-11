# Python: GPU device factory + real Screen capture + capture-encode bridge

`bindings/python/mediaway` — closes the same gap Node.js and C# closed first
(see [nodejs-gpu-device](nodejs-gpu-device.md), [csharp-gpu-device](csharp-gpu-device.md),
[language-bindings](language-bindings.md)): before this,
`VideoCapture.open(source="screen")` always passed a null/NONE GPU device
handle (no way to construct a real one) and unconditionally raised
`CaptureUnsupportedError`. `Window` capture keeps that same behavior — still
genuinely unreachable, no native constructor this pass.

## `_ffi.py` additions

`GpuAdapterInfo`/`GpuAdapterSelect`/`GpuDeviceOptions` `ctypes.Structure`s
mirror `mediaway_gpu_adapter_info_t`/`_select_t`/`_options_t` exactly (owned
`c_char_p` for the adapter name — the first owned-string ctypes field in this
binding; every other owned string field elsewhere is `U8P` + separate length).
Five new function prototypes: `mediaway_gpu_adapter_list`/`_list_free`,
`mediaway_gpu_device_create`/`_handle`/`_close`. `mediaway_gpu_adapter_list`'s
`mediaway_gpu_adapter_info_t **out_adapters` becomes `POINTER(POINTER(GpuAdapterInfo))`
— ctypes' direct double-pointer idiom, no manual byte-offset arithmetic needed.

## `GpuDevice` (new, in `_device.py`)

`GpuDevice.list_adapters()` (staticmethod, no instance needed) and
`GpuDevice.create(adapter_index=None, video_support=False, debug_layer=False)`
(classmethod) — `adapter_index=None` selects the backend's own default
adapter. `.handle` is the `_ffi.GpuDeviceHandle` value every existing consumer
(`mediaway_desktop_capture_config_screen`, `AutoVideoEncodeConfig.gpu_device`)
already accepted — no shape change needed at those call sites. Context-manager
support (`__enter__`/`__exit__`) matches every other handle-owning class here.

## `VideoCapture` changes

`open(source="screen", gpu_device=None)` creates a `GpuDevice` internally when
omitted (closed on `close()`); a caller-supplied device is left open (caller
owns it) — lets one device be shared between capture and
`AutoVideoEncoder.pick(gpu_device=...)`. `poll_frame()`/`size()`/
`release_frame()`/`close()` now branch on which capture domain opened the
session (`self._source`) — before this work `poll_frame()`/`size()` only ever
called the Camera-domain native functions, since Screen could never
successfully open to exercise the missing branches. `data` stays empty for
Screen's GPU-backed frames (no CPU pixel readback path in the wrapped Rust
backend); `poll_frame()` still proves real frames are arriving (`pts`,
negotiated geometry). Real pixels only ever move through the bridge below.

## Capture-to-encode bridge

`EncodeSession.write_frame_from_camera_capture(capture)` /
`.write_frame_from_desktop_capture(capture)` — poll-and-push in one native
call, no intermediate `VideoFrame`, no CPU copy for Screen's GPU frames
(`adr/pipeline/0005-capture-encode-bridge-c-abi.md`). Both reach into
`capture._handle` directly — Python has no assembly-boundary visibility problem
the way C# does (`InternalsVisibleTo` was needed there to cross
`Mediaway.Pipeline` → `Mediaway.Device.Camera`/`.Desktop`); one process, one
module graph, plain attribute access. `AutoVideoEncoder.pick()` gained
`pixel_format`/`bitrate_bps`/`gpu_device` keyword args (previously
codec/width/height/frame_rate only) — `pixel_format=PixelFormat.BGRA8` is
required for the Screen path since DXGI delivers BGRA8, not the NV12 default.

## Verified

Real hardware, this session: `GpuDevice.list_adapters()` enumerated the real
RTX 4090 + iGPU + WARP adapters; `VideoCapture.open(source="screen")` captured
5 real 2560x1440 GPU-backed frames (`examples/device/capture_screen.py`,
rewritten from a permanent-gap demo to a real run); the bridge itself reached
a real `AutoVideoEncoder.pick(gpu_device=...)` and
`write_frame_from_desktop_capture` call, gracefully hitting the same known
dev-machine WMF/DX11 GPU-input-encode limitation the Rust/C/Node.js/C#
siblings hit (`examples/pipeline/screen_record.py`, same rewrite). Python has
no capture/device test suite at all (unlike C#'s `CaptureTests.cs`) and none
of this is exercised in CI (`bindings/python`'s release RC gate only runs
`test_mux_roundtrip.py`/`test_decode_roundtrip.py`) — the two rewritten
examples are this binding's only verification surface for capture, matching
its existing convention.

## A stale doc claim fixed while touching `screen_record.py`

Its header claimed "no audio encoder exists in the ABI" — false since
`AudioEncoder` (ABI v2, `adr/0003-auto-audio-encode-c-abi.md`) already shipped
and `camera_record.py`'s own header already says "the old 'drained, not
muxed' gap is gone." Corrected the wording; did not add `screen_record.py`'s
own two-track remux (out of scope for this GPU-device pass, and `ScreenRecord.cs`
doesn't have one either — mic PCM stays drained, not muxed, here).
