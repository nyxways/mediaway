# ADR-0007: Quick Sync (`vpl-sys`) behind a `quicksync` cargo feature

- **Status**: Accepted
- **Date**: 2026-11-15
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder`

## Context

`vpl-sys` (oneVPL bindings for the Quick Sync backend, [quicksync ADR-0001](quicksync/0001-onevpl-quicksync-encode-surface.md))
runs `bindgen` in its build script, so it needs libclang on the build machine. It was a plain
`cfg(windows)` dependency, so **every** Windows consumer of `mediaway-encoder` needed libclang,
including one that only wants NVENC (whose `nvenc` crate is hand-written and has no build step).
A game that vendors the encoder for a screen-share stream hit exactly this.

## Decision

> A `quicksync` feature, **on by default**, owns `dep:vpl-sys` and the real Quick Sync backend.

| Feature | Enables |
|---------|---------|
| `quicksync` | `video` + `dep:vpl-sys` (Windows) and the real `quicksync::QuickSyncVideoEncoder` session |

- `default = ["full", "video", "quicksync"]`: nothing changes for current consumers.
- Off (or off Windows), `QuickSyncVideoEncoder` is the existing `Unsupported` stub: the type
  stays, so `auto::Backend::QuickSync`, `windows::auto::AutoVideoEncoder` and `support()` need no
  `cfg` (same pattern as `mediaway-device` ADR-0010). `support()` reports Quick Sync as
  `NotImplemented`, and `AutoHardwareOnly` skips it.
- Slim consumer: `default-features = false, features = ["video"]` builds without bindgen /
  libclang.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Generate the oneVPL bindings once and commit them | Removes libclang for everyone, but `vpl-sys`'s bindgen step pins struct layout to the vendored headers by design; a larger change than this need. |
| Gate the type and `Backend::QuickSync` too | Forces `cfg` into `auto` dispatch, the probe table and every consumer `match`. |

## Consequences

### Positive

- An NVENC/WMF-only Windows consumer builds without libclang.

### Negative / Trade-offs

- `cargo check --no-default-features --features video` reports Quick Sync as `NotImplemented`
  even on an Intel machine: a build-time choice, not a probe result.

## References

- [ADR-0004](0004-av-feature-gates.md) (audio / video features), `mediaway-device` ADR-0010,
  `mediaway-sw` ADR-0004
