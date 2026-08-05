# Browser: `DecodeSession` in `@mediaway/browser` (ADR-0022)

- Package: `bindings/browser/packages/browser` — `DecodeSession` (TS) wraps the
  browser's native WebCodecs `VideoDecoder`/`AudioDecoder`, fed by the wasm
  `Demuxer` (`crates/iso-bmff-wasm`). The decode-side mirror of `EncodeSession`
  (ADR-0020): WASM owns the container, the host owns codecs.
- NOT `mediaway-decoder-web`: the wasm32 crate backend
  (`mediaway-decoder::web`, video-only `decode_video_chunks` /
  `is_webcodecs_video_decode_supported`) stays a crate-level backend exercised
  by the `tools/e2e-web` raw-wasm harness; it is deliberately **not** a
  dependency of the npm package (same labor split ADR-0020 established for
  encode).
- Shape: `start()` picks the first video track (`h264|hevc|av1|vp9`) + first
  audio track (`aac|opus`) from `demuxer.streams()` (either may be absent),
  builds `VideoDecoderConfig`/`AudioDecoderConfig`, throws
  `DecoderUnavailableError` when the browser has no usable decoder or the
  config is unsupported; `pushPacket(sample)` routes by `streamId`; `finish()`
  flushes + closes. Output via single-listener `onVideoFrame`/`onAudioData`
  callbacks (the listener owns each frame/data and must close it).
- `resolveCodec(track): string` is required — why: `iso-bmff`'s `Track.codec`
  only stores the generic `Codec` name (`"h264"`/`"aac"`), not the WebCodecs
  profile/level string (`"avc1.42E01E"`/`"mp4a.40.2"`); the container format
  does not losslessly round-trip it. Round-trip callers know their own string;
  automatic derivation from `extraData` (avcC/ASC parsing) is deferred
  (ADR-0022 § Deferred).
- `AudioDecoderConfig.numberOfChannels` is not carried by `Track` — derived
  from the codec config bytes the container does store: AAC
  AudioSpecificConfig channelConfiguration / Opus OpusHead channel count.
- E2E: `tools/e2e-web/tests/browser-package.spec.ts` — H.264 + AAC
  encode→mux→demux→decode round trips via the real built package
  (`browser-package.html`); `msedge-real` project runs them, bundled-Chromium
  skips honestly (no H.264/AAC WebCodecs backend there).
- Docs: [`docs/adr/0022-browser-decode-session-and-device-dx.md`](../../../adr/0022-browser-decode-session-and-device-dx.md)
