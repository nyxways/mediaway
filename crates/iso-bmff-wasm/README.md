# iso-bmff-wasm

<p align="center">
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production; public APIs may
> change without notice. Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack.

WASM bindings for [`iso-bmff`](../iso-bmff/) mux/demux via `wasm-bindgen` — MP4
fragmented-mux and demux running in the browser, no C ABI involved.

## Quick start

```js
import init, { wasm_mux_av_bytes, wasm_mux_demux_smoke } from "./pkg/iso_bmff_wasm.js";

await init();
const mp4Bytes = wasm_mux_av_bytes(); // fMP4 bytes for a minimal H.264 + AAC mux
const packetCount = wasm_mux_demux_smoke(); // mux → demux round-trip
```

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| Mux exports (`Muxer`, `wasm_mux_av_bytes`, `wasm_mux_vp9_bytes`) | ✅ | H.264 + AAC, VP9 (`vp09`/`vpcC`) |
| Demux exports (`Demuxer`, smoke round-trips) | ✅ | |
| Browser E2E (`tools/e2e-web`) | ✅ | Playwright-driven |

## Docs

- [`iso-bmff`](../iso-bmff/) — the core this binds
- Root [README](../../README.md) — workspace overview

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
