# Node.js (TypeScript)

Server-side video processing and CLI tools call Mediaway through a native addon / N-API
(koffi FFI today) over the [`mediaway-ffi`](https://github.com/nyxways/mediaway/tree/main/crates/mediaway-ffi)
C ABI. Status: ✅ verified.

Examples live in [`bindings/nodejs/examples/`](https://github.com/nyxways/mediaway/tree/main/bindings/nodejs/examples):

| Capability | Example files |
|------------|---------------|
| Container | `container/mux-roundtrip.ts` |
| Device | `device/camera-record.ts` · `capture-microphone.ts` · `capture-screen.ts` |
| Pipeline | `pipeline/encode-audio.ts` · `encode-to-mp4.ts` · `screen-record.ts` |

Build and run instructions: [`bindings/nodejs/README.md`](https://github.com/nyxways/mediaway/blob/main/bindings/nodejs/README.md).
