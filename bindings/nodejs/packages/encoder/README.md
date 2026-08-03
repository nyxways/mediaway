# @mediaway/encoder

Automatic hardware video encoding to **fragmented MP4** over Mediaway's C ABI.
`openAutoEncoder()` picks a working backend for your machine (NVENC, Vulkan
Video, or QuickSync on Windows), wires it to the fMP4 muxer, and returns a
session you push frames into.

Windows is the verified platform. Software fallbacks and other platforms are
explicit follow-up work — see the [Mediaway roadmap](https://github.com/nyxways/mediaway).

## Install

```bash
npm install @mediaway/encoder
```

## Example

```ts
import { openAutoEncoder, AutoVideoEncodeConfig, EncoderUnavailableError, type VideoFrame } from "@mediaway/encoder";
import type { Rational } from "@mediaway/container";

const TIME_BASE: Rational = { num: 1, den: 30 };

const config = AutoVideoEncodeConfig.defaults("h264", 640, 480, TIME_BASE);
config.bitrateBps = 1_500_000; // override the backend default

let session;
try {
  session = await openAutoEncoder(config);
} catch (err) {
  if (err instanceof EncoderUnavailableError) {
    console.log("no hardware encoder available on this machine");
    process.exit(1);
  }
  throw err;
}

const greyFrame = Buffer.alloc(640 * 480 * 1.5); // NV12 (or i420/bgra8/rgba8)
for (let i = 0; i < 90; i++) {
  greyFrame.fill(i % 255);
  await session.writeFrame({
    data: greyFrame, // safe to reuse: the wrapper copies per writeFrame call
    width: 640,
    height: 480,
    pixelFormat: "nv12",
    pts: i,          // ticks of the config timeBase
  } satisfies VideoFrame);
}

const mp4 = await session.finish(); // terminal: complete fMP4 bytes
// -> write `mp4` to a file / stream
```

## API

| Member | Notes |
| --- | --- |
| `AutoVideoEncodeConfig.defaults(codec, width, height, timeBase)` | sensible defaults for the resolution; `codec` is `"h264" \| "hevc" \| "av1" \| "vp9"` |
| `openAutoEncoder(config)` | resolves a hardware backend or throws `EncoderUnavailableError` |
| `EncodeSession.writeFrame(frame)` | async; the frame is copied — reuse the buffer |
| `EncodeSession.finish()` | terminal; returns the complete fMP4 `Buffer` |

## License

MIT OR Apache-2.0. Part of the [Mediaway](https://github.com/nyxways/mediaway)
project — pre-1.0, APIs may change.
