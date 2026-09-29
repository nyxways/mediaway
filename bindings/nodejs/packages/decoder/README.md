# @mediaway/decoder

Automatic hardware video decode + Opus and AAC audio decode over Mediaway's C ABI.
Split out of `@mediaway/encoder` (previously buried as an undiscoverable
`decode.ts`) into its own peer package — decode and encode are siblings,
mirroring the Rust `mediaway-decoder`/`mediaway-encoder` crate split.

Windows is the verified platform for `DecodeSession` (WMF backend). Opus
audio decode (`AudioDecodeSession`) is cross-platform — `mediaway-sw`, no OS
dependency, identical output on every host.

**AAC decode** (`AudioDecodeSession.open({ codec: "aac", ... })`, ABI v7) is the
*operating system's* decoder: Media Foundation on Windows (run-verified) and
AudioToolbox on macOS/iOS (**compile-checked only — not run yet**); other platforms
answer status 4 (unsupported). Unlike Opus, its samples **can differ between hosts and OS
versions**. It needs the raw `AudioSpecificConfig` as `extraData` (an MP4's `esds`
DecoderSpecificInfo) — an empty one is refused (`MediawayError` status 5) rather than guessed,
because a synthesised default would decode SBR/PS streams to quietly wrong output. Raw AAC
only: strip ADTS headers first. An empty packet is also refused for AAC (it is Opus's
loss-concealment hint, and means nothing here).

```ts
import { AudioDecodeSession, decoderSupport } from "@mediaway/decoder";

if ((await decoderSupport("aac")) === "supported") {
  const aac = await AudioDecodeSession.open({
    codec: "aac",
    sampleRate: 48_000,
    channels: 2,
    extraData: audioSpecificConfig, // required; timeBase defaults to 1/sampleRate
  });
  for (const packet of aacPackets) await aac.pushPacket({ pts: packet.pts, payload: packet.data });
  await aac.flush();
  for (let f; (f = await aac.pollFrame()); ) consume(f.data); // interleaved f32le PCM
  aac.close();
}
```

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
| `AudioDecodeSession.open(sampleRate, channels, timeBase)` | Opus (the original positional form); cross-platform |
| `AudioDecodeSession.open({ codec, sampleRate, channels, timeBase?, extraData? })` | `"opus"` (needs `timeBase`) or `"aac"` (needs `extraData`; OS decoder) |
| `decoderSupport(codec)` | async, **costly** (opens a throwaway session); `"supported" \| "not-implemented" \| "no-device"` — how to learn whether AAC decode exists here |

## License

MIT OR Apache-2.0. Part of the [Mediaway](https://github.com/nyxways/mediaway)
project — pre-1.0, APIs may change.
