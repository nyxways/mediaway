# ADR-0022: Browser decode session (video + audio) + device DX parity

- **Status**: Accepted
- **Date**: 2026-08-06
- **Deciders**: @dev-nyxie (+ agent)
- **Amends**: [ADR-0020](0020-browser-wasm-npm-package.md) (`@mediaway/browser`)

## Context

ADR-0020 shipped `@mediaway/browser` with real container (WASM `Muxer`/`Demuxer`) and
WebCodecs encode (`EncodeSession`), but explicitly deferred "video decode-to-canvas
pipelines" to a follow-up ADR once a concrete consumer appeared. That consumer is now
this audit: the package can produce an fMP4 but has no way to play one back — an
asymmetric, incomplete round trip for a media library.

Two more gaps surfaced auditing "web completeness":

1. **Audio decode does not exist anywhere in the browser path** — not in the package,
   not even as a wasm-crate smoke export. `mediaway-decoder-web` (the wasm32 backend of
   the `mediaway-decoder` facade) only implements video decode
   (`decode_video_chunks`/`is_webcodecs_video_decode_supported` in `src/web/wasm.rs`),
   used today only by the `tools/e2e-web` test harness, never by the shipped npm
   package.
2. **Device capture has no enumerate/hotplug example.** ADR-0020 already decided device
   capture stays native (`getUserMedia`/`getDisplayMedia`, not wrapped) — that stands.
   But the native stack's device capability (ADR-0005, `mediaway-device`) includes
   `DeviceId`/`Select`/hotplug vocabulary with an enumerate + change-notification story;
   the browser binding's examples never demonstrate the equivalent
   (`navigator.mediaDevices.enumerateDevices()` + the `devicechange` event), so a
   consumer copying the examples has no DX parity for "list devices, react to
   plug/unplug."

Also found while auditing (doc drift, not a design question, fixed alongside this ADR
rather than getting its own): `crates/mediaway-decoder/docs/roadmap.md` Stage 2 (Web)
was fully unchecked despite `src/web/` already existing; `docs/ai/wiki/platform/web-encode.md`
listed `GPUTexture` Zero-Copy under "Next" even though `webgpu_canvas_frame` /
`webcodecs_gpu_video_fmp4_smoke` already implement and E2E-verify it; the same wiki page
called non-H.264 codecs "probe-only" even though `tools/e2e-web/tests/codec-support-matrix.spec.ts`
already runs real HEVC/AV1/VP9 encode→decode round trips (not just a capability probe).

## Decision

### 1. `DecodeSession` in `@mediaway/browser` — video + audio

Mirror `EncodeSession`'s labor split exactly, in reverse: TS wraps the browser's native
`VideoDecoder`/`AudioDecoder` directly, fed by the WASM `Demuxer`. **`mediaway-decoder-web`
is not pulled into the package** — same reasoning ADR-0020 already established for
`mediaway-encoder-web` (codecs are host-native; the WASM module's only job is the
container). `mediaway-decoder-web` stays what it is today: an `mediaway-decoder`
platform backend exercised by its own crate tests and the `tools/e2e-web` harness, not a
dependency of the npm package. Future readers should not read "decode is now in the
package" as "therefore port the wasm crate in" — it is a deliberate non-dependency, not
an oversight.

Shape (mirrors `EncodeSession`'s typestate):

```ts
interface DecodeSessionConfig {
  /**
   * Map a demuxed Track to the exact WebCodecs codec string (e.g. "avc1.42E01E",
   * "mp4a.40.2"). Required: `iso-bmff`'s `Track.codec` only carries the generic
   * `Codec` name ("h264"/"aac"/...), not the profile/level string WebCodecs needs to
   * configure a decoder — the container format doesn't losslessly round-trip it, so
   * the caller (who already knows what it encoded, or reads it from its own
   * out-of-band metadata) supplies it. No guessing/derivation inside the package.
   */
  resolveCodec(track: Track): string;
}

class DecodeSession {
  constructor(demuxer: Demuxer, config: DecodeSessionConfig);
  /** Reads streams() (first video + first audio track), configures decoders. */
  async start(): Promise<void>;
  onVideoFrame(cb: (frame: VideoFrame) => void): void;
  onAudioData(cb: (data: AudioData) => void): void;
  /** Feed one demuxed packet in (streamId routes to the right decoder). */
  pushPacket(sample: Sample): void;
  /** Flush both decoders. */
  async finish(): Promise<void>;
}

class DecoderUnavailableError extends Error {}
```

- Track selection: first video track + first audio track, symmetric with
  `EncodeSession`'s single-video/single-audio scope. Multi-track is out of scope here
  (no current consumer), same as encode.
- `VideoDecoderConfig`/`AudioDecoderConfig`'s `codec` string comes from
  `resolveCodec(track)`; `description` comes from the demuxed `Track.extraData` (avcC /
  `AudioSpecificConfig`) — the decode-side mirror of `EncodeSession` deferring
  `addTrack` until the encoder's `description` arrives. Deriving the precise codec
  string from `extraData` automatically (e.g. parsing avcC's profile/constraint/level
  bytes into `avc1.PPCCLL` for H.264) is a real, well-defined future addition but out of
  scope here — it only covers H.264 cleanly, HEVC/AV1/VP9 need real bitstream parsing,
  and no current consumer needs it since round-trip callers already know their own
  codec string.
- Errors: `DecoderUnavailableError` mirrors `EncoderUnavailableError` — thrown when
  `VideoDecoder`/`AudioDecoder` is undefined or `isConfigSupported` reports false, not
  swallowed into a silent no-op.
- Undecided at this ADR's scope, left to implementation + review: whether frame
  delivery is callback-based (`onVideoFrame`) or an async iterator. Callbacks match
  `EncodeSession`'s existing `output`/`error` shape from WebCodecs itself; keep the
  simpler option unless a concrete consumer needs backpressure.

### 2. Device: examples only, no new wrapper

Confirms ADR-0020's "capture stays native" decision — add
`examples/device/list-and-watch-devices.ts` demonstrating
`navigator.mediaDevices.enumerateDevices()` + the `devicechange` event, with a comment
noting this is the browser-native analog of `mediaway-device`'s `DeviceId`/`Select`/
hotplug vocabulary (ADR-0005). Update the capability table in
[`bindings/browser/README.md`](../../bindings/browser/README.md) to list it. No Rust
change, no new npm package class.

### 3. Doc-drift fixes bundled with this pass

- `crates/mediaway-decoder/docs/roadmap.md` Stage 2 (Web): check off "Add
  `mediaway-decoder-web`" and "WebCodecs decode + `VideoFrame`" — both already true.
- `docs/ai/wiki/platform/web-encode.md`: remove `GPUTexture` Zero-Copy from "Next" (it
  shipped); correct the "other video codecs probe-only" line — HEVC/AV1/VP9 already get
  real round-trip E2E coverage (`codec-support-matrix.spec.ts`), only the *browser's own*
  codec support varies (honest per-codec skip, not a Mediaway gap).
- `docs/ai/wiki/decode/index.md`: add a pointer to `web-video-decode.md` if missing, and
  a new short page for `DecodeSession` once implemented (English, ≤100 lines, per Rule 0
  wiki upkeep).

## Alternatives Considered

| Alternative | Why not |
|---|---|
| Port `mediaway-decoder-web`'s wasm decode functions into the npm package | Breaks the "WASM = container only, WebCodecs = codecs" split ADR-0020 already established for encode; duplicates the native `VideoDecoder`/`AudioDecoder` calls the browser already exposes for free. |
| Wrap device capture in a `Mediaway.Device`-style browser class | ADR-0020 already decided against wrapping capture (native Web APIs, not wrapped) — no new information here changes that; only the missing enumerate/hotplug *example* was a real gap. |
| Async-iterator-only frame delivery for `DecodeSession` | No concrete backpressure consumer yet; callbacks keep parity with `EncodeSession` and WebCodecs' own callback shape. Revisit if a streaming-playback consumer needs it. |

## Consequences

- **Positive**: `@mediaway/browser` gets a full encode→mux→demux→decode round trip,
  closing ADR-0020's deferred item; device examples reach DX parity with the native
  stack's enumerate/hotplug story without adding wrapper surface area; three stale docs
  get corrected as a side effect of the audit that produced this ADR.
- **Negative / Trade-offs**: `DecodeSession` inherits the same WebCodecs
  availability variance as encode (must fail with `DecoderUnavailableError`, never
  silently no-op); audio decode via `AudioDecoder` needs its own PCM-shape handling,
  analogous to `AudioData` on the encode side, first exercised only in this package (no
  existing `mediaway-decoder-web` audio precedent to mirror).
- **Deferred**: multi-track decode, seek/random-access, canvas-rendering helpers beyond
  handing back raw `VideoFrame`/`AudioData` (rendering is the consumer's job, same as
  encode never wrapped canvas capture beyond the existing capture examples), and
  automatic codec-string derivation from `extraData` (H.264-only would be well-defined;
  HEVC/AV1/VP9 need real bitstream parsing) — `resolveCodec` is the explicit escape
  hatch until a consumer needs it.

## References

- [ADR-0020](0020-browser-wasm-npm-package.md) — the package this amends
- [`docs/adr/0005` (crate-local, `mediaway-device`)](../../crates/mediaway-device/adr/0005-device-selection.md) —
  `DeviceId`/`Select`/hotplug vocabulary this ADR's device example mirrors in native
  Web APIs
- `crates/mediaway-decoder/src/web/wasm.rs` — existing WebCodecs decode wasm exports
  (video-only), confirmed NOT a dependency of this ADR's `DecodeSession`
- `tools/e2e-web/tests/decode-trim-splice.spec.ts`,
  `tools/e2e-web/tests/codec-support-matrix.spec.ts` — existing real decode-side E2E
  coverage this ADR's package-level `DecodeSession` gets its own new spec alongside
- [`bindings/browser/README.md`](../../bindings/browser/README.md) — the DX contract
  this ADR extends
