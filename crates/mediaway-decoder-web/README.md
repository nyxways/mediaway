# mediaway-decoder-web

WebCodecs `VideoDecoder` decode for `wasm32-unknown-unknown` (browser).

Requires `--cfg=web_sys_unstable_apis` (set in [`.cargo/config.toml`](../../.cargo/config.toml) for wasm32).

H.264/VP9/AV1 decode are verified against a real, separately-installed system browser (`msedge-real` Playwright project, not just the bundled Chromium) — not a fallback-loop guess. HEVC decode is 🆗 on real Edge only (no other browser tested ships an HEVC WebCodecs decoder).

E2E: [`tools/e2e-web`](../../tools/e2e-web/README.md). Full per-codec matrix + real-browser findings: [`docs/ai/wiki/decode/web-video-decode.md`](../../docs/ai/wiki/decode/web-video-decode.md).
