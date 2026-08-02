# Browser E2E (Playwright)

Chromium-first Playwright specs for WASM mux and WebCodecs encode paths.

## Setup

```bash
rustup target add wasm32-unknown-unknown
cd tools/e2e-web
bun install
bun run build:wasm
bun run install-browsers
bun test
```

`build:wasm` compiles `iso-bmff-wasm`, `mediaway-encoder-web`, `mediaway-decoder-web`, and
`mediaway-device-web` for `wasm32-unknown-unknown` and runs `wasm-bindgen` into `pkg/`.

## Projects: bundled Chromium vs. real Edge

Two Playwright projects, both defined in `playwright.config.ts`:

- **`chromium`** (default, `bun run test`) — Playwright's bundled Chromium. Fast, but has no
  real H.264/AAC WebCodecs encode/decode backend, so H.264/AAC-gated specs honestly
  `test.skip()` (`webcodecs-fmp4.spec.ts`) or fall back to VP9 (`decode-trim-splice.spec.ts`).
- **`msedge-real`** (`bun run test:real-edge`) — the machine's real, separately-installed
  system Microsoft Edge (Chromium-based, ships with Windows) via Playwright's `channel:
  "msedge"`, which resolves to the system binary with no extra browser download. Edge has a
  genuine H.264/AAC WebCodecs backend, so the same specs run the real codec path here instead
  of skipping/falling back — `decode-trim-splice.spec.ts` pins the codec to H.264 only on this
  project (see its top-of-file comment). See
  `docs/ai/wiki/encode/web-real-chrome-bugs.md` for bugs previously found only reachable this
  way.

`test:real-edge` requires Edge installed at its default Windows path; it is not part of the
default `bun run test` / CI job.

## Codec support matrix (HEVC / AV1 / VP9)

`tests/codec-support-matrix.spec.ts` checks and reports WebCodecs encode + decode support for
HEVC, AV1, and VP9 individually (no fallback loop) on both projects, and runs a real
encode-decode round trip for every codec that reports support. Empirically:
encode-supported codecs also round-trip successfully — no case yet of `isConfigSupported`
over-reporting a codec that then fails the round trip (unlike H.264 previously, see
`docs/ai/wiki/decode/web-video-decode.md`).

## CI

Optional `e2e-web` job in `.github/workflows/ci.yml` (not a pre-push gate).

## Fake media

Headless capture tests use Chromium flags:

- `--use-fake-ui-for-media-stream`
- `--use-fake-device-for-media-stream`

Real picker flows remain manual.
