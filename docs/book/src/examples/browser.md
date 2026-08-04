# Browser (WASM + WebCodecs)

Browser apps run Mediaway natively in the page — WASM (`wasm-bindgen`) wired into
WebCodecs and WebGPU, bypassing the C ABI entirely for zero-overhead browser execution.
Status: ✅ verified.

Examples live in [`bindings/browser/examples/`](https://github.com/nyxways/mediaway/tree/main/bindings/browser/examples):

| Capability | Example files |
|------------|---------------|
| Container | `container/mux-roundtrip.ts` |
| Device | `device/camera-record.ts` · `capture-microphone.ts` · `capture-screen.ts` |
| Pipeline | `pipeline/encode-audio.ts` · `encode-to-mp4.ts` · `screen-record.ts` |

These are exercised end-to-end by Playwright against `iso-bmff-wasm` and the WebCodecs
backends. Build and run instructions: [`bindings/browser/README.md`](https://github.com/nyxways/mediaway/blob/main/bindings/browser/README.md).
