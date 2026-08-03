# mediaway-encoder-web

WebCodecs H.264/AAC encode for `wasm32-unknown-unknown` (browser).

Requires `--cfg=web_sys_unstable_apis` (set in [`.cargo/config.toml`](../../.cargo/config.toml) for wasm32).

H.264/AAC encode is 🆗 via WebCodecs. A GPU-resident path (WebGPU canvas → `VideoFrame`, no CPU readback) also exists and is verified on the real-Edge Playwright project, but stays 🆗 not ⚡ since browser-internal GPU sharing is unobservable from JS/wasm.

E2E: [`tools/e2e-web`](../../tools/e2e-web/README.md). Full per-codec matrix + real-browser findings: [`docs/ai/wiki/decode/web-video-decode.md`](../../docs/ai/wiki/decode/web-video-decode.md).
