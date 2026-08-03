# @mediaway/browser

Browser (WASM) bindings for Mediaway — **fMP4 mux and demux** compiled to
WebAssembly (wasm-bindgen, ~130 KB wasm, size-optimized with wasm-opt `-Oz`).
No Node.js and no native DLL: pure WebAssembly.

## Install

```bash
npm install @mediaway/browser
```

## Example

```ts
import { init, Muxer, Demuxer, type Track, type Sample } from "@mediaway/browser";

// init() fetches and instantiates the wasm; nothing else may run before it.
// In the browser, `await init()` alone resolves the packaged wasm URL.
await init();

const muxer = new Muxer(); // fragmentBatch: samples per fMP4 fragment (>= 1)
const trackId = muxer.addTrack({
  codec: "h264",
  width: 640,
  height: 480,
  timeBase: { num: 1, den: 30 },
} satisfies Track);

muxer.begin();
for (let i = 0; i < 90; i++) {
  muxer.pushSample({
    trackId,
    data: encodedBytes(i), // Uint8Array
    pts: i,
    duration: 1,
    key: i % 30 === 0,
  } satisfies Sample);
}
const mp4 = muxer.end(); // complete fMP4 bytes -> Blob / File

// Demux the same bytes:
const demuxer = new Demuxer();
demuxer.pushBytes(mp4);
const streams = demuxer.streams(); // Track[] once moov parsed
for (;;) {
  const sample = demuxer.pollSample(); // Sample | null
  if (!sample) break;
  // sample.trackId, sample.data, sample.pts, sample.duration, sample.key
}
```

## Notes

- `init()` must complete before any other call. In the browser it resolves
  the packaged wasm URL automatically; under Node pass the wasm bytes
  explicitly (file: fetch is unavailable there).
- The same `Muxer`/`Demuxer` shape is available as a `@mediaway/container`
  class with a koffi-backed implementation for Node.js.

## License

MIT OR Apache-2.0. Part of the [Mediaway](https://github.com/nyxways/mediaway)
project — pre-1.0, APIs may change.
