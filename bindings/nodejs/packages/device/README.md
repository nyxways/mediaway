# @mediaway/device

Camera, microphone, and screen capture over Mediaway's C ABI
(`mediaway_ffi.dll`). Capture is **poll-based and synchronous** — frames are
ready when `pollFrame()` returns one; it never blocks waiting for hardware.

Windows is the verified platform (Media Foundation backends). Linux camera
backends compile; other platforms are not implemented yet.

## Install

```bash
npm install @mediaway/device
```

## Camera example

```ts
import { openCamera, CaptureUnavailableError, type VideoFrame } from "@mediaway/device";

const camera = await openCamera({ index: 0, timeBase: { num: 1, den: 30 } });
console.log(`camera: ${camera.width}x${camera.height}`);

const frames: Buffer[] = [];
const started = Date.now();
while (Date.now() - started < 3_000) {
  const frame = camera.pollFrame(); // VideoFrame | null — never blocks
  if (frame) frames.push(Buffer.from(frame.data)); // NV12 planes
}
await camera.close(); // joins the backend worker thread (up to one frame interval)
```

Handle a missing camera cleanly:

```ts
try {
  const camera = await openCamera({ index: 0, timeBase: { num: 1, den: 30 } });
  // ... record
} catch (err) {
  if (err instanceof CaptureUnavailableError) console.log("no camera here");
  else throw err;
}
```

## Microphone

```ts
import { openMicrophone } from "@mediaway/device";

const mic = await openMicrophone({ sampleRate: 48_000, channels: 1 });
// mic.sampleRate, mic.channels
const frame = mic.pollFrame(); // AudioFrame | null — raw interleaved f32le PCM
await mic.close();
```

## Screen capture

GPU-only, Zero-Copy — there is no CPU fallback in the wrapped Rust backend
(`adr/0003-gpu-handle-c-abi.md` §4), so `openScreenCapture()` needs a live GPU
device. It creates one internally by default (via the GPU device factory,
`mediaway-device` ADR-0007):

```ts
import { openScreenCapture } from "@mediaway/device";

const screen = await openScreenCapture({ timeBase: { num: 1, den: 30 }, monitorIndex: 0 });
console.log(`screen: ${screen.width}x${screen.height}`);
const frame = screen.pollFrame(); // VideoFrame | null — data is ALWAYS empty (see below)
await screen.close(); // also closes the internally-created GPU device
```

`pollFrame()`'s `VideoFrame.data` is always empty — it proves frames are
genuinely arriving (real `pts`/geometry) but does not copy pixels out (no CPU
readback path for Screen). For real pixel data, feed the session straight
into an encoder with `EncodeSession.writeFrameFromDesktopCapture()`
(`@mediaway/encoder`), Zero-Copy end to end — see that package's README.

## GPU device factory

```ts
import { listGpuAdapters, GpuDevice, openScreenCapture } from "@mediaway/device";

const adapters = listGpuAdapters(); // name, vendorId, deviceId, dedicatedVideoMemoryBytes, isHardware
const device = await GpuDevice.create({ adapterIndex: adapters[0].index });

// Share one device across capture and encode (see @mediaway/encoder):
const screen = await openScreenCapture({ timeBase: { num: 1, den: 30 }, gpuDevice: device });
// ... screen.close() leaves `device` open since you supplied it — close it yourself:
await screen.close();
device.close();
```

Omit `adapterIndex` (or the whole `options` object) for the first hardware
adapter DXGI reports.

## API

| Function | Session | Frames |
| --- | --- | --- |
| `openCamera({ index, timeBase })` | `CameraSession` (`width`, `height`, `pixelFormat` = `"nv12"`) | `pollFrame(): VideoFrame \| null` |
| `openMicrophone({ sampleRate, channels })` | `MicSession` (`sampleRate`, `channels`) | `pollFrame(): AudioFrame \| null` |
| `openScreenCapture({ timeBase, monitorIndex?, gpuDevice? })` | `ScreenSession` | `pollFrame(): VideoFrame \| null` (`data` always empty) |
| `listGpuAdapters()` | — | `GpuAdapterInfo[]` |
| `GpuDevice.create({ adapterIndex?, videoSupport?, debugLayer? })` | `GpuDevice` | — |

Every session's `close()` is async (joins the backend worker thread).
`CaptureUnavailableError` signals a missing device.

## License

MIT OR Apache-2.0. Part of the [Mediaway](https://github.com/nyxways/mediaway)
project — pre-1.0, APIs may change.
