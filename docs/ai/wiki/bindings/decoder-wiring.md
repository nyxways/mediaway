# Decode session wiring (C++ → C# → Python → Node)

## What existed vs. what was wired

The pipeline C ABI's decode sessions — `mediaway_decode_session_*` (auto video
decode, `adr/0004-auto-decode-c-abi.md`) and `mediaway_audio_decode_session_*`
(Opus audio decode, `adr/pipeline/0006-audio-decode-c-abi.md`) — have existed
since v0.1.4. No language binding had wired either one: the exact same
"C ABI real, binding missing" gap the [container format
series](status.md#container-format-wiring-all-8-formats-c-c-python-nodejs)
closed for mux/demux. This series closes it for decode, same language order.

Both sessions mirror `AutoVideoEncoder`/`AudioEncoder`'s single-step shape
(the handle IS the decoder, no consumption trap); `NoBackend` is a graceful,
expected outcome (WMF video decode is Windows-only; Opus decode is
cross-platform via `mediaway-sw`, no OS dependency).

## C++ (done)

`decoder::DecodeSession` / `decoder::AudioDecodeSession` in
`bindings/cpp/include/mediaway/pipeline.hpp`, following the existing
`encoder::AutoVideoEncoder`/`AudioEncoder` RAII pattern exactly (same
`unique_ptr` + custom-deleter shape, same `NoBackend` exception path via
`detail::checkPipeline`). Added `Status::DecodeError` +
`MEDIAWAY_PIPELINE_STATUS_DECODER_BACKEND_FAILURE`/`_CLOSED` mapping to
`core.hpp`/`pipeline.hpp`'s status switch (previously unmapped — any decode
backend failure would have fallen through to the generic `EncodeError`
case).

Verified end-to-end: `examples/pipeline/decode_roundtrip.cpp` — a real WMF
H.264 encode → `container::Demuxer` → `DecodeSession` round trip (10 frames,
extra_data sourced from the demuxed AVCC track, not straight off the
encoder — the shape a caller decoding a received file would actually have),
plus a real Opus encode (raw C ABI — the C++ `AudioEncoder` wrapper is
AAC-only, not extended here) → `AudioDecodeSession` round trip (50 frames).
Linked and ran against a freshly built `mediaway_ffi.dll`
(`x86_64-pc-windows-gnu`), not just compiled.

## C# (done)

`DecodeSession` / `AudioDecodeSession` in `Mediaway.Pipeline`, mirroring
`AutoVideoEncoder`/`AudioEncoder`'s `SafeHandle` pattern exactly (both new
handles have no consumption trap, unlike `AutoEncoderHandle`/
`EncodeSessionHandle` — closer to `AudioEncodeSessionHandle`'s shape).
Declarations added to both `NativeMethods.LibraryImport.cs` (net8.0) and
`NativeMethods.DllImport.cs` (netstandard2.0/Unity) — the dual-TFM split
ADR-0018 introduced. `MediawayPipelineStatus` gained
`DecoderBackendFailure`/`DecoderClosed` (13/14); a new
`DecoderUnavailableException` mirrors `EncoderUnavailableException` for the
graceful `NoBackend` case.

Verified end-to-end: `DecodeRoundtripTests` in `Mediaway.Pipeline.Tests` — a
real WMF H.264 encode→mux→demux→decode round trip (10 frames) and a real
Opus encode→decode round trip (50 frames). The Opus encode side used a
test-local raw P/Invoke declaration rather than the internal
`NativeMethods` (no `InternalsVisibleTo` to the test project — the public
`AudioEncoder` wrapper is AAC-only, same gap as C++'s), following the same
precedent `Mediaway.Device`'s hardware capture tests set for a test-only raw
P/Invoke.

## Python (done)

`DecodeSession` / `AudioDecodeSession` in a new `mediaway/_decoder.py`
module, mirroring `AutoVideoEncoder`/`AudioEncoder`'s single-step-handle
`ctypes` shape from `_encoder.py`. `_check_pipeline` (shared with
`_encoder.py`) gained a `no_backend_error=` parameter so `NO_BACKEND` raises
the new `DecoderUnavailableError` instead of `EncoderUnavailableError`.

Notable find: Python's `AudioEncoder.open()` already took a `codec=`
parameter (defaulting to AAC, but Opus was always a valid argument) —
unlike C++/C#'s wrappers, which hardcode AAC and needed a raw-ABI workaround
to test Opus decode. `AudioEncoder.open(codec=Codec.OPUS, ...)` just worked,
no extra plumbing needed.

Verified end-to-end: `examples/pipeline/decode_roundtrip.py` (narrated) and
`tests/test_decode_roundtrip.py` (RC-stage assert-based script, no pytest
dependency, same style as `test_mux_roundtrip.py`) — a real WMF H.264
encode→mux→demux→decode round trip (10 frames) and a real Opus
encode→decode round trip (50 frames).

## Node (pending)

Not yet wired. Same shape expected: Node koffi, plus the same class of
proactive checks the container series found repeatedly — missing
mirror-enum variants, stale native DLL shadowing a fresh build, double-free
on a consumed handle.
