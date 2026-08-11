# Node.js: GPU device factory + real Screen capture + capture-encode bridge

`bindings/nodejs` — landed ahead of C#/Python/C++ (see
[language-bindings](language-bindings.md)). Closes the same gap the C ABI's
own `mediaway-ffi` GPU device factory closed (`mediaway-device` ADR-0007):
before it, `@mediaway/device`'s `openScreenCapture()` unconditionally threw
`CaptureUnavailableError` — no Node caller had any way to construct or own a
GPU device, and Screen capture has no CPU fallback.

## Package split

Five packages now (was four): `@mediaway/ffi`, `@mediaway/container`,
`@mediaway/decoder`, `@mediaway/encoder`, `@mediaway/device`.
`@mediaway/decoder` is new — `decode.ts` moved out of `@mediaway/encoder`
(previously undiscoverable: no separate README, not mentioned in the
encoder's own docs despite being part of its public exports). Decode and
encode are peer capabilities, mirroring the Rust `mediaway-decoder`/
`mediaway-encoder` crate split — neither package depends on the other, each
duplicates its own small `checkPipeline`/`PixelFormat`/`VideoCodec`
definitions rather than one importing from the other.

## `@mediaway/device` additions

- `listGpuAdapters()` — enumerates every DXGI adapter (name, VRAM,
  hardware-vs-software), wrapping `mediaway_gpu_adapter_list`.
- `GpuDevice.create(options?)` — `{ adapterIndex?, videoSupport?, debugLayer? }`;
  omit `adapterIndex` for the first hardware adapter DXGI reports. Wraps
  `mediaway_gpu_device_create`/`_handle`/`_close`.
- `openScreenCapture({ timeBase, monitorIndex?, gpuDevice? })` — creates a
  `GpuDevice` internally when `gpuDevice` is omitted (and closes it on
  `ScreenSession.close()`); a caller-supplied device is left open (caller
  owns it) — lets one device be shared between capture and encode.
- `ScreenSession.pollFrame()` proves frames are genuinely arriving (real
  `pts`/geometry) but `VideoFrame.data` is always empty — there is no CPU
  pixel readback path for Screen in the wrapped Rust backend either (GPU-only
  storage, `adr/0003-gpu-handle-c-abi.md` §4). Real pixels only ever move
  through the bridge below.

## Capture-to-encode bridge

`EncodeSession.writeFrameFromCameraCapture(capture)` /
`.writeFrameFromDesktopCapture(capture)` — poll-and-push in one native call,
no intermediate `VideoFrame`, no CPU copy for Screen's GPU frames
(`adr/pipeline/0005-capture-encode-bridge-c-abi.md`). `AutoVideoEncodeConfig`
gained a `gpuDevice` field (set together with `pixelFormat: "bgra8"` — DXGI
delivers BGRA8, not the NV12 default) so the auto encoder negotiates a
GPU-input-capable backend before the bridge is ever called.

## Cross-package native handle sharing

`CameraSession`/`ScreenSession`/`GpuDevice` all wrap an opaque native handle
in a `private` field — but `@mediaway/encoder` (a separate npm package) needs
to read it for the bridge and for `gpuDevice` config. Rather than a plain
public getter (raw ABI pointer in the public API, against this binding's own
rule), `@mediaway/device` exports one `unique symbol`, `NATIVE_HANDLE`, and
each class implements `[NATIVE_HANDLE](): unknown` as a symbol-keyed method.
`@mediaway/encoder` imports the same symbol and calls
`capture[NATIVE_HANDLE]()` — reachable only by importing a clearly-`@internal`
-documented symbol, not discoverable via autocomplete on the object itself.

## Verified

`examples/device/capture-screen.ts` (5 real 1920x1080 frames polled) and
`examples/pipeline/screen-record.ts` (real screen + mic capture; GPU-input
H.264 encode itself gracefully skips as `UNSUPPORTED` on this dev machine's
current encoder/driver — the same pre-existing limitation
`gpu_write_frame_smoke.rs`/the C `screen_record.c` hit, not introduced here)
both run-verified on real hardware.

## An unrelated bug found while verifying

`bun`'s default workspace linking is per-package/"isolated" — it does not
hoist `@mediaway/*` into the repo-root `node_modules` the way npm workspaces
do. That silently broke root-level `tsc --noEmit`/`tsx` module resolution for
`test/*.ts` (pre-existing, reproduced even on files untouched by this work —
`mux-roundtrip.test.ts`, `all-formats-smoke.test.ts`). Fixed with
`bindings/nodejs/bunfig.toml`'s `[install] linker = "hoisted"`, which matches
what the package manager recipe (`bun install && npm test`,
`bindings/nodejs/README.md`) always assumed.
