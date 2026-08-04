# Node.js (TypeScript)

Server-side video processing and CLI tools call Mediaway through a native addon / N-API
(koffi FFI today) over the [`mediaway-ffi`](https://github.com/nyxways/mediaway/tree/main/crates/mediaway-ffi)
C ABI. Status: ✅ verified.

## Install

```bash
npm install @mediaway/container   # also @mediaway/device, @mediaway/encoder, @mediaway/ffi
```

```ts
import { Muxer } from "@mediaway/container";

const muxer = new Muxer();
const videoTrack = muxer.addVideoTrack({
  codec: "h264", width: 1920, height: 1080,
  pixelFormat: "nv12", timeBase: { num: 1, den: 30 },
});
const chunks: Buffer[] = [muxer.begin()]; // init segment (ftyp + moov), write once

muxer.push({
  trackIndex: videoTrack,
  data: Buffer.from([0, 0, 0, 1]),
  pts: 0, duration: 1, key: true,
});
muxer.flush();
for (let chunk = muxer.pollBytes(); chunk.length > 0; chunk = muxer.pollBytes()) {
  chunks.push(chunk);
}
```

Examples live in [`bindings/nodejs/examples/`](https://github.com/nyxways/mediaway/tree/main/bindings/nodejs/examples):

| Capability | Example files |
|------------|---------------|
| Container | `container/mux-roundtrip.ts` |
| Device | `device/camera-record.ts` · `capture-microphone.ts` · `capture-screen.ts` |
| Pipeline | `pipeline/encode-audio.ts` · `encode-to-mp4.ts` · `screen-record.ts` |

Build and run instructions: [`bindings/nodejs/README.md`](https://github.com/nyxways/mediaway/blob/main/bindings/nodejs/README.md).
