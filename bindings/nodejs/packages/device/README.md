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

```ts
import { openScreenCapture } from "@mediaway/device";

const screen = await openScreenCapture({
  timeBase: { num: 1, den: 30 },
  captureCursor: true,
});
const frame = screen.pollFrame(); // VideoFrame | null
await screen.close();
```

## API

| Function | Session | Frames |
| --- | --- | --- |
| `openCamera({ index, timeBase })` | `CameraSession` (`width`, `height`, `pixelFormat` = `"nv12"`) | `pollFrame(): VideoFrame \| null` |
| `openMicrophone({ sampleRate, channels })` | `MicSession` (`sampleRate`, `channels`) | `pollFrame(): AudioFrame \| null` |
| `openScreenCapture({ timeBase, captureCursor? })` | `ScreenSession` | `pollFrame(): VideoFrame \| null` |

Every session's `close()` is async (joins the backend worker thread).
`CaptureUnavailableError` signals a missing device.

## License

MIT OR Apache-2.0. Part of the [Mediaway](https://github.com/nyxways/mediaway)
project — pre-1.0, APIs may change.
