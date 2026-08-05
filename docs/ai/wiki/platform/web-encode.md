# Web encode (WebCodecs)

- Module: `mediaway-encoder::web` (wasm32 + `web_sys_unstable_apis`)
- CPU path: NV12 `VideoFrame` + `AudioData` → WebCodecs H.264/AAC → `iso-bmff` mux
- Probe: `is_webcodecs_video_codec_supported(codec)` for `avc1` / `hev1` / `av01` / `vp09` strings
- README: H.264/AAC encode 🆗; other video codecs per-browser — real
  encode→decode round trips where the browser ships them
  (`tools/e2e-web/tests/codec-support-matrix.spec.ts`, honest per-codec skip
  otherwise)
- GPUTexture Zero-Copy: shipped — `webgpu_canvas_frame` /
  `webcodecs_gpu_video_fmp4_smoke` in `crates/mediaway-encoder/src/web/wasm.rs`,
  E2E-verified in `tools/e2e-web/tests/webcodecs-fmp4.spec.ts`
- E2E: `tools/e2e-web/tests/webcodecs-fmp4.spec.ts`
- Decode: `@mediaway/browser` `DecodeSession` (ADR-0022) — TS wrapper over
  WebCodecs `VideoDecoder`/`AudioDecoder` fed by the wasm `Demuxer`; see
  [decode/web-decode-session](../decode/web-decode-session.md)
- Next: none open here — see ADR-0022 § Deferred (multi-track decode,
  seek/random-access, automatic codec-string derivation)
