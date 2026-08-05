# mediaway-container-ffi — roadmap

C ABI facade over `mediaway-container`. Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 1 — Surface design

- [x] ADR: opaque handle types, error code enum, function naming, memory
      ownership (who allocates/frees buffers), header layout
      (`adr/0001-*.md`)
- [x] Scaffold only so far: `Cargo.toml` (`cdylib`/`staticlib`/`rlib`),
      empty `src/lib.rs`

### 2 — Mux surface

- [x] Opaque `Muxer` handle: open, add_track, begin, push_packet, flush,
      poll_bytes, close
- [x] Generated/hand-written `mediaway_container.h`

### 3 — Demux surface

- [x] Opaque `Demuxer` handle: open, push_bytes, streams, poll_packet, close
- [x] ClearKey decrypt (`mediaway_demuxer_set/clear_decryption_key`) and a custom
      fragment-batch muxer constructor (`mediaway_muxer_create_with_fragment_batch`) —
      `adr/0002-clearkey-decrypt-and-fragment-batch-c-abi.md`

### 4 — CI + consumer smoke test

- [x] CI builds at least one slim feature set
- [x] A minimal C (or C++) program links + round-trips mux→demux, matching
      `bindings/c/examples/mux_roundtrip.c`'s scenario (verified via
      `--target x86_64-pc-windows-gnu` + `gcc`: 90 video + 90 audio packets
      recovered exactly)

### Deferred

- `cbindgen` migration — tooling adopted crate-wide
  (`docs/adr/0016-cbindgen-ffi-headers.md`'s 2026-08-05 addendum); this
  module's `include/mediaway/container.h` itself is not yet migrated (still
  hand-written).
