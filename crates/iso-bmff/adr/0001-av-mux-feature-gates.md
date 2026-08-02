# ADR-0001: Audio / video / mux / demux Cargo features

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `iso-bmff`

## Context

Audio-only apps (podcast, voice) should not compile or link video bitstream helpers (AVCC, H.264 paths). Mux-only recorders may omit demux (and `iso-cenc` decrypt). Symmetric slim graphs matter for WASM and native.

## Decision

> Optional Cargo features on `iso-bmff` (forwarded by `mediaway-container`):

| Feature | Enables |
|---------|---------|
| `audio` | AAC ADTS strip, audio codec registration/mux paths |
| `video` | H.264 AVCC, video codec registration/mux paths |
| `mux` | [`Muxer`](../../src/mux/mod.rs) |
| `demux` | [`Demuxer`](../../src/demux/mod.rs) + `iso-cenc` decrypt |
| `full` | `audio` + `video` + `mux` + `demux` |

- **Default:** `full` (backward compatible).
- Slim use: `default-features = false`, e.g. `features = ["audio", "mux"]`.
- Disabled codecs return [`Error::InvalidPacket`](../../src/error.rs) at track/packet registration.

## Consequences

- CI and apps must pass features explicitly when disabling defaults.
- One muxer type remains; features gate codec paths, not separate crates.

## References

- [`docs/spec/crate-packaging.md`](../../../docs/spec/crate-packaging.md)
- [`docs/spec/c-ffi.md`](../../../docs/spec/c-ffi.md) (feature-gated capabilities)
