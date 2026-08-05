# mediaway-ffi — roadmap

C ABI facade over `mediaway`'s auto video encode -> fragmented MP4 convenience layer. Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 1 — Surface design

- [x] ADR: opaque handle types, config struct (which `AutoVideoEncodeConfig`
      fields ship in v1 vs deferred), error code enum, function naming,
      memory ownership, header layout (`adr/0001-*.md`)
- [x] Scaffold only so far: `Cargo.toml` (`cdylib`/`staticlib`/`rlib`),
      empty `src/lib.rs`

### 2 — Auto-encode surface

- [x] Opaque encode-session handle: open (config in, graceful "unsupported
      on this platform" out), write_frame, finish, close
- [x] Hand-written `mediaway_pipeline.h` (or similar)

### 3 — CI + consumer smoke test

- [ ] CI builds cleanly
- [x] A minimal C (or C++) program links + encodes synthetic frames to
      `out.mp4`, matching `bindings/c/examples/encode_to_mp4.c`'s scenario
      (verified via `--target x86_64-pc-windows-gnu` + `gcc`: real WMF H.264
      encode, valid 6253-byte MP4)

### Deferred (not this crate's first pass)

- ~~Screen/camera capture (`platform::ScreenCapture`/`Microphone`)~~ — raw
  capture itself is real and hardware-verified via this crate's `device`
  module (`adr/device/0001-capture-c-abi.md`); the capture-to-encode
  convenience bridge is also now implemented and hardware-verified
  (`adr/pipeline/0005-capture-encode-bridge-c-abi.md`,
  `mediaway_encode_session_write_frame_from_{camera,desktop}_capture`,
  ABI v4) — real USB camera → real WMF H.264 → valid fMP4,
  `tests/capture_encode_bridge_smoke.rs`. Microphone audio composition into
  the same session is still the caller's job (out of this ADR's scope).
- ~~Decode (`platform::AutoDecoder`)~~ — C ABI implemented
  (`adr/pipeline/0004-auto-decode-c-abi.md`, `mediaway_decode_session_*`,
  ABI v3), compiles/clippy clean, but its own integration test
  (`tests/decode_smoke.rs`) is `#[ignore]`d — blocked on a real, pre-existing
  `WindowsVideoDecoder` CPU-decode bug found while writing it, not a defect
  in this FFI wrapper (`docs/ai/wiki/platform/windows-decode.md` § CPU
  decode bug).
- `cbindgen` migration — tooling adopted crate-wide
  (`docs/adr/0016-cbindgen-ffi-headers.md`'s 2026-08-05 addendum); this
  module's `include/mediaway/pipeline.h` itself is not yet migrated (still
  hand-written).
- ~~Opus audio decode + Opus reachable from the audio encode C ABI~~ —
  implemented (`adr/pipeline/0006-audio-decode-c-abi.md`,
  `mediaway_audio_decode_session_*` + `mediaway_audio_encode_config_opus`,
  ABI v5), hardware-independent (`mediaway-sw`'s pure-Rust backend, no OS
  dependency), round-trip-verified (`tests/audio_decode_smoke.rs`).
  Sequence tracking / jitter buffering / FEC scheduling for a real-time voice
  transport stay out of scope — application-side, not a codec concern.
