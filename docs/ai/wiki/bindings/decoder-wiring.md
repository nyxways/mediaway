# Decode session wiring (C++ → C# → Python → Node) — DONE, all 4

## What existed vs. what was wired

The pipeline C ABI's decode sessions — `mediaway_decode_session_*` (auto video
decode, `adr/0004-auto-decode-c-abi.md`) and `mediaway_audio_decode_session_*`
(Opus audio decode, `adr/pipeline/0006-audio-decode-c-abi.md`) — have existed
since v0.1.4. No language binding had wired either one: the exact same
"C ABI real, binding missing" gap the [container format
series](status.md#container-format-wiring-all-8-formats-c-c-python-nodejs)
closed for mux/demux. This series closes it for decode, same language order,
now complete: all 4 bindings reach both decode sessions.

Both sessions mirror `AutoVideoEncoder`/`AudioEncoder`'s single-step shape
(the handle IS the decoder, no consumption trap); `NoBackend` is a graceful,
expected outcome (WMF video decode is Windows-only; Opus decode is
cross-platform via `mediaway-sw`, no OS dependency). Every binding's public
`AudioEncoder`-equivalent wrapper stayed AAC-only, so the Opus round-trip
test/example in each language encodes via a raw-ABI path instead of the
public encoder wrapper — see each section for how.

## C++

`decoder::DecodeSession` / `decoder::AudioDecodeSession` in
`bindings/cpp/include/mediaway/pipeline.hpp`, mirroring `encoder::` RAII
(`unique_ptr` + deleter, `NoBackend` via `detail::checkPipeline`). Added
`Status::DecodeError` + `DECODER_BACKEND_FAILURE`/`_CLOSED` mapping
(previously unmapped, fell through to `EncodeError`). Opus encoded via raw
C ABI in the example itself. Verified: `examples/pipeline/decode_roundtrip.cpp`
— WMF H.264 encode→mux→demux→decode (10 frames) + Opus encode→decode (50
frames), linked and run against a fresh `mediaway_ffi.dll`.

## C#

`DecodeSession` / `AudioDecodeSession` in `Mediaway.Pipeline`, mirroring
`SafeHandle` pattern (closer to `AudioEncodeSessionHandle`'s no-consumption-
trap shape). Declared in both `NativeMethods.LibraryImport.cs` (net8.0) and
`.DllImport.cs` (netstandard2.0/Unity, ADR-0018). `MediawayPipelineStatus`
gained `DecoderBackendFailure`/`DecoderClosed` (13/14); new
`DecoderUnavailableException`. Opus encoded via a test-local raw P/Invoke
(no `InternalsVisibleTo` to the test project — same precedent
`Mediaway.Device`'s hardware tests set). Verified: `DecodeRoundtripTests` in
`Mediaway.Pipeline.Tests` — same 10-frame/50-frame round trips.

## Python

`DecodeSession` / `AudioDecodeSession` in a new `mediaway/_decoder.py`,
mirroring `_encoder.py`'s single-step `ctypes` shape. `_check_pipeline`
gained a `no_backend_error=` parameter → `DecoderUnavailableError`. Notable:
Python's `AudioEncoder.open()` already took `codec=` (Opus was always a
valid argument, unlike C++/C#'s hardcoded-AAC wrappers) — `AudioEncoder.open
(codec=Codec.OPUS, ...)` just worked, no raw-ABI workaround needed. Verified:
`examples/pipeline/decode_roundtrip.py` + `tests/test_decode_roundtrip.py`
(RC-stage, assert-based, no pytest) — same round trips.

## Node

`DecodeSession` / `AudioDecodeSession` in a new
`packages/encoder/src/decode.ts`, mirroring `index.ts`'s single-step koffi
shape. `@mediaway/ffi` gained the decode structs/functions; `checkPipeline`
(exported from `encoder/index.ts`) gained a `noBackendError` parameter.
Same AAC-only gap as C++/C#: Opus encoded via the raw `@mediaway/ffi`
`pipeline` object directly — no test-local P/Invoke needed since
`@mediaway/ffi` is already a real workspace package, unlike C#'s
internals-gated `NativeMethods`. Verified: `examples/pipeline/decode-roundtrip.ts`
+ `test/decode-roundtrip.test.ts` (`node:assert`, wired into `npm test`) —
same round trips; `tsc --noEmit` across the workspace passes clean.
