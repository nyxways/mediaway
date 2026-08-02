# ADR-0004: Audio / video Cargo features

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder`

## Context

Audio-only products should not pull video encoder traits/config into their dependency graph. Platform backends (`mediaway-encoder-windows`, `mediaway-encoder-web`) mirror the same features.

## Decision

| Feature | Enables |
|---------|---------|
| `audio` | [`AudioEncoder`](../../src/audio.rs), [`AudioEncoderConfig`](../../src/audio.rs) |
| `video` | [`VideoEncoder`](../../src/video.rs), [`VideoEncoderConfig`](../../src/video.rs), [`auto`](../../src/auto.rs) |
| `full` | `audio` + `video` |

Default: `full`. Slim: `default-features = false`, e.g. `features = ["audio"]`.

## References

- [`0001-encoder-traits.md`](0001-encoder-traits.md)
- `iso-bmff` ADR-0001 (mux feature gates)
