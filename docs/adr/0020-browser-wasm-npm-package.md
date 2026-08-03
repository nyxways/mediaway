# ADR-0020: Browser WASM binding — `@mediaway/browser` npm package

- **Status**: Accepted
- **Date**: 2026-08-03
- **Deciders**: @dev-nyxie (+ agent)

## Context

The browser is a **Tier C host** ([`docs/spec/c-ffi.md`](../spec/c-ffi.md) § Tier C):
it reaches Mediaway through **WASM (`wasm-bindgen`) + native Web APIs, never the C ABI**.
`crates/iso-bmff-wasm` already proves the sans-io container core runs in-browser, but
only via test-only smoke exports (`wasm_mux_demux_smoke` etc.) — there is no
`@mediaway/browser` package, and the binding folder is 📐 design
([`bindings/browser/README.md`](../../bindings/browser/README.md)). The wiki open item
asks for a "crate-category ADR + wasm-bindgen API design" before the package is built.

The C-ABI hosts (C/C++/Python/Node, plus C#) now have a full pipeline surface including
audio encode (`mediaway-pipeline-ffi/adr/0003`, ABI v2). The browser host has no
equivalent because **WASM cannot ship OS/hardware codec backends** — and it does not
need to: the browser platform itself provides codecs via **WebCodecs**
(`VideoEncoder`/`AudioEncoder`) and capture via `getUserMedia`/`getDisplayMedia`.

## Decision

Ship a real `@mediaway/browser` npm package (TypeScript-first) with this labor split:

1. **Container (WASM, ours)**: promote `crates/iso-bmff-wasm` from smoke exports to the
   real container API — `Muxer`/`Demuxer` `wasm_bindgen` classes mirroring the Rust
   typestate (`Muxer::new` → `addTrack` → `begin` → `pushPacket` → `flush` →
   `pollBytes`; `Demuxer::new` → `pushBytes` → `streams` → `pollPacket`). `Uint8Array`
   in/out (copied), explicit `.free()` on every WASM-side object.
2. **Codecs (WebCodecs, browser-native)**: the package wires WebCodecs encode output
   into the WASM muxer — `AutoVideoEncoder` + terminal `EncodeSession` (the browser
   analog of `mediaway-pipeline`'s auto-encode session), plus an `AudioEncoder`
   wrapper. The WASM module **never** ships or encodes codec bitstreams; Mediaway owns
   the pipeline + container, the platform owns codecs. Audio: WebCodecs `AudioEncoder`
   (browser AAC/Opus); the `AudioSpecificConfig` for the mux track comes from the
   encoder's `description` — the browser analog of the C-ABI `stream_info()` call order.
3. **Capture (native Web APIs, not wrapped)**: `getUserMedia()` (camera/mic) and
   `getDisplayMedia()` (screen) feed frames straight into `writeFrame`.

### Crate category

`iso-bmff-wasm` stays **one crate with one job**: the WASM adapter of the freestanding
`iso-bmff` core (naming v1, ADR-0012 — unprefixed cores keep their own platform
adapters). No new crate category is introduced; the "WASM binding" category is a
**thin wasm-bindgen adapter co-located with the core it binds**, same relationship
`mediaway-container::mp4` has to `iso-bmff` on the native side. `publish.workspace`
reason flips: the container API is browser-verified; codecs are WebCodecs-native, so
nothing in the browser path is unverifiable-by-design anymore.

### Package layout

```
bindings/browser/
  packages/browser/            # the npm package (name: @mediaway/browser)
    package.json               # publish-ready (build via prepack script)
    src/                       # TS wrapper: init, Muxer, Demuxer, encoder, session
    pkg/                       # wasm-pack output — gitignored, built by the script
  examples/                    # mirror the native examples' sector layout
```

Build: `wasm-pack build crates/iso-bmff-wasm --target web` → `bindings/browser/packages/browser/pkg/`,
driven by a Bun script under `tools/scripts/` (repo convention) and wired into
`prepack` so `npm publish` always ships a fresh wasm artifact.

### API contract (DX)

- `await init()` (idempotent) before anything else; after it, calls are synchronous.
- Typed config objects mirror the Rust types: `Track` (id/codec/time_base/width/
  height/extraData), `Sample` (streamId/pts/dts/duration/isKeyframe/isDiscard/payload),
  `Rational` (`{ num, den }`), codec as lowercase string (`"h264" | "hevc" | "av1" |
  "vp9" | "aac" | "opus" | "webvtt" | "tx3g"`).
- `Muxer`: `addTrack(track) -> trackId` (duplicates the typestate: pushes go to the
  open muxer), `begin()`, `pushPacket(sample)`, `flush()`, `pollBytes(): Uint8Array`
  (full accumulated output, fresh copy each call), `free()`.
- `Demuxer`: `pushBytes(bytes)`, `streams(): Track[]`, `pollPacket(): Sample | null`,
  `free()`.
- WebCodecs glue: `VideoEncoder`/`AudioEncoder` constructors take a codec
  (`"avc1.42E01E"`-style or `"mp4a.40.2"`), the wasm `Muxer`, and a `Rational` timebase;
  `EncodeSession` collects `description`/`extra_data` before the first media packet and
  passes it to `addTrack` — the browser analog of the C-ABI push → stream_info → mux
  order (ADR-0003).
- `Error` subclasses for expected failures (`EncoderUnavailableError`), matching the
  C-ABI status-code philosophy translated to exceptions.

### Verification

- Node smoke: the built wasm (web target) also runs under Node — mux → demux
  roundtrip with synthetic H.264 + AAC packets, asserting ftyp + packet counts.
- Browser E2E: real Chromium (the harness browser) loads a page using the package,
  WebCodecs-encodes synthetic frames, muxes, demuxes, and reports counts — proving the
  full browser path, not just the wasm unit.

## Alternatives Considered

| Alternative | Why not |
|---|---|
| Ship codecs in WASM (transpile/port C codecs to wasm) | Duplicates what the browser platform ships natively (WebCodecs), adds megabytes + GPL/license risk — contradicts the vision's "OS/GPU offload, small binaries". The browser's codec set *is* the user's browser's codec set; Mediaway adds pipeline + container. |
| Wrap the C ABI in the browser (emscripten the `*-ffi` cdylibs) | Tier C explicitly forbids the C ABI in browsers ([`docs/spec/c-ffi.md`](../spec/c-ffi.md) § Tier C); the `-ffi` crates carry OS/GPU backends that do not compile to wasm (WMF/DXGI/…). |
| Build the container into the browser package from TS (hand-rolled fMP4 writer) | Re-implements `iso-bmff` in TS — duplicated logic, no shared tests, drift risk. The Rust core is the SSOT; `iso-bmff-wasm` is a thin adapter. |
| New dedicated `mediaway-browser-wasm` crate for the wasm exports | Splits the adapter away from the core it binds with no packaging benefit; `iso-bmff-wasm` already exists and is the natural home (naming v1). |

## Consequences

- **Positive**: the browser folder graduates from 📐 design to ✅ verified for
  container + WebCodecs-encode scenarios; `@mediaway/browser` is npm-publishable with
  the same metadata discipline as the other bindings; the browser's audio encode is
  real (WebCodecs `AudioEncoder`) without any wasm codec work.
- **Negative / Trade-offs**: WebCodecs availability varies by browser/version — the
  package must fail with `EncoderUnavailableError`, not silently no-op; screen capture
  needs Chromium's Insertable-Streams path; WASM objects need explicit `.free()`
  (JS GC cannot see into wasm memory) — documented on every class.
- **Deferred**: WebGPU zero-copy texture handoff (encode-from-`GPUTexture`), video
  decode-to-canvas pipelines, `MediaStreamTrackProcessor` screen record verification on
  non-Chromium engines. Each gets its own ADR when a concrete consumer appears.

## References

- [`docs/spec/c-ffi.md`](../spec/c-ffi.md) § Tier C — browser is WASM, never the C ABI
- [`docs/spec/crate-packaging.md`](../spec/crate-packaging.md) — facades / backends /
  unprefixed cores + naming v1 (ADR-0012)
- `crates/iso-bmff-wasm/README.md` — smoke-exports status being promoted by this ADR
- `crates/mediaway-pipeline-ffi/adr/0003-auto-audio-encode-c-abi.md` — the push →
  stream_info → mux call order the browser audio path mirrors
- [`bindings/browser/README.md`](../../bindings/browser/README.md) — the DX contract
  this ADR implements
