# mediaway-sw-opus — roadmap

Pure Rust Opus encode/decode (`unsafe-libopus`), isolated unsafe boundary — separate from
`mediaway-sw`, which stays `#![forbid(unsafe_code)]`.
Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 0 — Scaffold

- [x] Crate + `docs/` / `adr/`
- [x] ADR: dependency review + unsafe-boundary + crate-placement decision —
  [`adr/0001-unsafe-libopus-encode-decode.md`](../adr/0001-unsafe-libopus-encode-decode.md)
  (**Status: Accepted** — real `cargo deny check` against `unsafe-libopus`'s transitive
  graph ran clean; `unsafe-libopus = "0.2"` added to `[workspace.dependencies]`)

### 1 — Opus encode

- [x] Add `unsafe-libopus` to `[workspace.dependencies]`; `cargo deny check` ran clean
- [x] `OpusEncoder`: `open` / `stream_info` / `push_frame` / `poll_packet` / `flush` (RAII
  over `*mut OpusEncoder`, `Drop` → `opus_encoder_destroy`), method shape mirrors
  `mediaway_encoder::AudioEncoder`
- [x] In-band FEC / expected-packet-loss-percent config (`opus_encoder_ctl!`)
- [x] Frame-size + `SampleFormat::F32`-only input validation
- [x] Real encode test: encode a synthetic PCM tone, assert non-trivial compressed output
  (no committed media — `docs/conventions/testing.md`)
- [x] Wired as `mediaway_encoder::AudioEncoder` SW fallback — `WindowsAudioEncoder` dispatches
  `CodecKind::Opus` to `SwOpusAudioEncoder` (`crates/mediaway-encoder/src/windows/mod.rs`)

### 2 — Opus decode

- [x] `OpusDecoder`: `open` / `stream_info` / `push_packet` / `poll_frame` / `flush`, method
  shape mirrors `mediaway-decoder-windows`'s `WmfOpusDecoder` session
- [x] Real decode test, round-tripped against this crate's own `OpusEncoder` output
  (`tests/roundtrip.rs` — RMS energy similarity check, lossy codec so not byte-exact)
- [ ] Wire behind an `AudioDecoder` trait once `mediaway-decoder` defines one (out of scope
  here — same gap `mediaway-decoder-windows/src/wmf/opus.rs` already names)

### 3 — Cost documentation

- [x] rustdoc cost note on `push_frame`/`poll_packet`/`push_packet`/`poll_frame`: payload
  copy across the raw pointer boundary + upstream's ~20% CPU cost vs. the C reference (no
  inline asm/SIMD in the transpile) — per
  [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md)
