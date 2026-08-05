# Opus SW crate — `mediaway-sw::opus`

- Path: `crates/mediaway-sw` — module `mediaway-sw::opus`
- **Status: encode + decode implemented** ([ADR-0001](../../../../crates/mediaway-sw/adr/opus/0001-unsafe-libopus-encode-decode.md),
  Accepted) — `OpusEncoder`/`OpusDecoder`, 18 unit tests + a round-trip integration test
  (`crates/mediaway-sw/tests/opus/roundtrip.rs`, RMS-energy check on a decoded sine wave). Not yet wired into a
  public `mediaway-encoder`/`mediaway-decoder` trait.
- Root `README.md`'s CPU/SW table now marks Opus 🆗 (was 👻 "No pure Rust stack targeted",
  then 🛠️ while Proposed)

## Why not inside `mediaway-sw`

`mediaway-sw` is `#![forbid(unsafe_code)]`, crate-wide, no exceptions — a real, tested
invariant (`mediaway-sw` ADR-0001). `unsafe-libopus`'s public functions
(`opus_encoder_create`, `opus_encode_float`, `opus_decoder_create`, `opus_decode_float`, …)
are **`unsafe fn`** — C-shaped, raw `*mut OpusEncoder`/`*mut OpusDecoder` pointers, manual
create/destroy. Unlike `rav1e` (`mediaway-sw`'s AV1 dependency, fully safe public API), this
one genuinely needs `unsafe` glue code to wrap. The wrapper gets its own crate with a single
crate-root `#![allow(unsafe_code)]` (mirrors `vpl-sys`: the whole crate's purpose is this one
FFI boundary, so the allow lives once at the root rather than per-module) + `// SAFETY:` on
every `unsafe` block.

## Why it matters

- `CodecKind::Opus` already exists in `mediaway-common`, but before this crate **no encode
  path existed on any platform** — Windows has no inbox Opus encoder MFT at all (confirmed
  via a real `MFTEnumEx` query, `crates/mediaway-decoder/src/windows/wmf/opus.rs`).
- The one real Opus **decode** path (`WmfOpusDecoder`, same file) is hardware-verified and
  now implements `mediaway-decoder`'s `AudioDecoder` trait ([ADR-0003](../../../../crates/mediaway-decoder/adr/0003-audio-decoder-trait.md)).
- `unsafe-libopus` (crates.io, `DCNick3/unsafe-libopus`) — BSD-3-Clause, already on
  `deny.toml`'s allow-list, `c2rust`-transpiled libopus 1.3.1, IETF test-vector conformant,
  no system libopus / CMake / autotools build step. `cargo deny check` against the resolved
  graph is clean.

## Design (as implemented)

`config.rs` holds `OpusApplication` + `OpusEncoderConfig`/`OpusDecoderConfig` +
`frame_size_samples` (shared by both sessions); `error.rs` holds the single `OpusError`
`thiserror` enum both directions use; `encoder.rs`/`decoder.rs` hold `OpusEncoder`/
`OpusDecoder`. `OpusEncoder` mirrors `mediaway_encoder::AudioEncoder`'s push/poll method
names; `OpusDecoder` mirrors `WmfOpusDecoder`'s (`push_packet`/`poll_frame`/`flush`).
Neither `impl`s the sibling facade trait directly — this crate does not depend on
`mediaway-encoder`/`mediaway-decoder` to avoid an unwanted dependency edge for a leaf
codec crate. Both facades instead wrap these sessions in a thin newtype
(`SwOpusAudioEncoder` in `mediaway-encoder`, `SwOpusAudioDecoder` in `mediaway-decoder`'s
`audio::sw_opus`) that implements the local trait and maps `OpusError` onto the facade's
own error type.

Both sessions own a private raw pointer with RAII `Drop` → `opus_{encoder,decoder}_destroy`,
and a justified `unsafe impl Send` (not `Sync`) — the raw pointer never appears in the public
API. `push_frame`/`push_packet` accept only `SampleFormat::F32` (matches `opus_encode_float`
and the WMF decoder's own F32 output). `OpusEncoderConfig::time_base` / `OpusDecoderConfig::
time_base` double as the Opus frame duration in seconds (`num/den`, e.g. `Rational::new(1,
50)` for 20ms) — combined with `sample_rate` this fixes the exact PCM sample count per call;
mismatches are a hard `OpusError::FrameSizeMismatch`/`Backend` reject, never re-buffered.
Both sessions reuse per-call scratch buffers (`pcm_scratch`/`packet_scratch`) rather than
allocating fresh `Vec`s each call — only the final owned `Packet`/`AudioFrame` payload copy
is unavoidable. Real, upstream-acknowledged ~20% CPU cost vs. the C reference (no inline
asm/SIMD in the transpile) is documented on `OpusEncoder`/`OpusDecoder`'s own rustdoc, not
just here — see [`caveats-and-clarity.md`](../../../spec/caveats-and-clarity.md)'s catalog.
