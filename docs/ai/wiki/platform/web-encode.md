# Web encode (WebCodecs)

- Crate: `mediaway-encoder-web` (wasm32 + `web_sys_unstable_apis`)
- CPU path: NV12 `VideoFrame` + `AudioData` → WebCodecs H.264/AAC → `iso-bmff` mux
- Probe: `is_webcodecs_video_codec_supported(codec)` for `avc1` / `hev1` / `av01` / `vp09` strings
- README: H.264/AAC encode 🆗; other video codecs probe-only (🛠️ until smoke encode)
- E2E: `tools/e2e-web/tests/webcodecs-fmp4.spec.ts`
- Next: `GPUTexture` Zero-Copy · HEVC/AV1/VP9 encode smoke
