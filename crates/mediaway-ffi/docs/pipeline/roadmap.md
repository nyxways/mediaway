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

- Screen/camera capture (`platform::ScreenCapture`/`Microphone`) — separate
  capability, own ADR once this encode-only surface is real
- Decode (`platform::AutoDecoder`) — separate capability
