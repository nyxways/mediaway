# Browser (WASM + WebCodecs)

Browser apps run Mediaway natively in the page — WASM (`wasm-bindgen`) wired into
WebCodecs and WebGPU, bypassing the C ABI entirely for zero-overhead browser execution.
Status: ✅ verified.

## Install

```bash
npm install @mediaway/browser
```

```ts
import { init, Muxer } from "@mediaway/browser";

await init(); // fetches + instantiates the WASM module

const muxer = new Muxer(); // one sample per fragment
muxer.addTrack({
  id: 0, codec: "h264",
  timeBase: { num: 1, den: 30 }, width: 1920, height: 1080,
  extraData: new Uint8Array(/* avcC … */),
});
muxer.begin(); // live: packets may now be pushed
muxer.pushPacket({
  streamId: 0, pts: 0, dts: 0, duration: 1,
  isKeyframe: true, isDiscard: false,
  payload: new Uint8Array([0, 2, 0x65, 0x88]),
});
muxer.flush();
const mp4 = muxer.pollBytes(); // fresh copy into JS-owned memory
muxer.free(); // WASM handle — JS GC cannot see into WASM memory
```

Examples live in [`bindings/browser/examples/`](https://github.com/nyxways/mediaway/tree/main/bindings/browser/examples):

| Capability | Example files |
|------------|---------------|
| Container | `container/mux-roundtrip.ts` |
| Device | `device/camera-record.ts` · `capture-microphone.ts` · `capture-screen.ts` |
| Pipeline | `pipeline/encode-audio.ts` · `encode-to-mp4.ts` · `screen-record.ts` |

These are exercised end-to-end by Playwright against `iso-bmff-wasm` and the WebCodecs
backends. Build and run instructions: [`bindings/browser/README.md`](https://github.com/nyxways/mediaway/blob/main/bindings/browser/README.md).
