# Web: WebGPU-backed `VideoFrame` (Stage 2 GPU path)

`mediaway-encoder::web` `wasm.rs`: `webcodecs_gpu_video_fmp4_smoke` / `is_webgpu_video_frame_supported`.

## Key finding — no `GPUTexture` → `VideoFrame` constructor

WebCodecs' `VideoFrame` constructor only accepts a `CanvasImageSource`
(`HTMLCanvasElement` / `OffscreenCanvas` / `ImageBitmap` / `HTMLVideoElement` / …). A raw
`GPUTexture` is **not** a member of that union — confirmed empirically on Chromium 131/148
(headless, this repo's Playwright builds): `new VideoFrame(texture, { timestamp: 0 })` throws
`TypeError: Overload resolution failed`. `web-sys` 0.3.103's bindings (same IDL) expose no
constructor taking a bare `GpuTexture` either.

## Supported GPU-resident path

1. `OffscreenCanvas` + `getContext("webgpu")` → `GPUCanvasContext`.
2. `ctx.configure({ device, format })`; write into `ctx.getCurrentTexture()` via a render
   pass (or any WebGPU draw/compute targeting that texture) — GPU-resident, no CPU buffer.
3. `VideoFrame::new(canvas, { timestamp })` — accepted; confirmed working empirically.

## Honesty label

No `Vec<u8>` / `copyTo` / `GPUBuffer` readback in the Mediaway/wasm code. Whether the
browser's internal `VideoFrame` *shares* the canvas's compositor texture or does its own
internal GPU→GPU copy is implementation-defined and unobservable from JS/wasm — documented as
**GPU-resident, no CPU readback in the Mediaway path**, not an unconditional Zero-Copy claim.
Catalog row: [caveats-clarity](../meta/caveats-clarity.md) (`webgpu_canvas_frame`).

## `web_sys_unstable_apis`

All `Gpu*` / WebGPU `web-sys` bindings require `--cfg=web_sys_unstable_apis` (already set for
`wasm32-unknown-unknown` in `.cargo/config.toml`).

## Test env note

This project's `@playwright/test` (1.49.1) pins Chromium build **1148**; a stray
1223/1228 download did not match it (`channel: "chromium"` in `playwright.config.ts` selects
the full build over `chromium-headless-shell`, which has no GPU process). `navigator.gpu` +
adapter/device were independently verified working on that build. H.264 **encode** is
*reported* supported by `isConfigSupported`, but a real encode fails in this build (both for
this WebGPU-canvas source and for a plain CPU NV12 frame) — see
[decode/web-video-decode](../decode/web-video-decode.md) § "Second real bug" for the full
root cause and fix (`video_codec_supported` now runs one real encode as a follow-up check, not
just `isConfigSupported`). `is_webgpu_video_frame_supported()` therefore now correctly reports
`false` here, and `webcodecs_gpu_video_fmp4_smoke`'s smoke test skips honestly instead of
throwing `OperationError: Encoding error`. `webgpu_canvas_frame`'s render pass itself was never
the problem.

## Real Chrome via CDP confirms the WebGPU path genuinely works

Manually launched the machine's installed Google Chrome (a real, non-Playwright build) with
`--remote-debugging-port` + `--remote-allow-origins=*` and drove it with
`playwright-core`'s `chromium.connectOverCDP` — **run the driving script under Node, not
Bun**: Bun's CDP WebSocket transport hung until Playwright's 30s connect timeout in every
attempt (same script, same flags, succeeded immediately under Node). On that real browser,
H.264 encode is genuinely supported and `webcodecs_gpu_video_fmp4_smoke` produced a real
one-packet fMP4 end to end — the Playwright-bundled Chromium's missing H.264 encoder was a
test-environment limit, never a bug in this crate's WebGPU/H.264 code.

That same real-Chrome session also surfaced three independent, real bugs invisible on the
Playwright build (which never reaches these paths): see
[web-real-chrome-bugs](web-real-chrome-bugs.md) for all three and their fixes.
