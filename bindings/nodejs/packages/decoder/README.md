# @mediaway/decoder

Automatic hardware video decode + Opus audio decode over Mediaway's C ABI.
Split out of `@mediaway/encoder` (previously buried as an undiscoverable
`decode.ts`) into its own peer package — decode and encode are siblings,
mirroring the Rust `mediaway-decoder`/`mediaway-encoder` crate split.

Windows is the verified platform for `DecodeSession` (WMF backend). Opus
audio decode (`AudioDecodeSession`) is cross-platform — `mediaway-sw`, no OS
dependency.

## Install

```bash
npm install @mediaway/decoder
```

## Example

```ts
import { DecodeSession, DecoderUnavailableError, type DecodedVideoFrame } from "@mediaway/decoder";
import type { Rational } from "@mediaway/container";

const TIME_BASE: Rational = { num: 1, den: 30 };

let session;
try {
  session = await DecodeSession.open({
    codec: "h264",
    width: 640,
    height: 480,
    timeBase: TIME_BASE,
    extraData: avccBytes, // AVCC / SPS-PPS, required at open time
  });
} catch (err) {
  if (err instanceof DecoderUnavailableError) {
    console.log("no hardware decoder available on this machine");
    process.exit(1);
  }
  throw err;
}

for (const packet of compressedPackets) {
  await session.pushPacket(packet);
  let frame: DecodedVideoFrame | null;
  while ((frame = await session.pollFrame()) !== null) {
    // consume frame.data (frame.pixelFormat, width, height)
  }
}
await session.flush();
session.close(); // always safe — no consumption trap
```

## API

| Member | Notes |
| --- | --- |
| `DecodeSession.open(config)` | resolves a hardware video decoder or throws `DecoderUnavailableError` |
| `DecodeSession.pushPacket(packet)` / `pollFrame()` | may produce zero or more frames per pushed packet |
| `AudioDecodeSession.open(sampleRate, channels, timeBase)` | Opus only today; cross-platform |

## License

MIT OR Apache-2.0. Part of the [Mediaway](https://github.com/nyxways/mediaway)
project — pre-1.0, APIs may change.
