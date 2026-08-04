# mediaway-test-media

<p align="center">
  <a href="https://docs.rs/mediaway-test-media"><img src="https://img.shields.io/docsrs/mediaway-test-media" alt="docs.rs"></a>
  <a href="https://crates.io/crates/mediaway-test-media"><img src="https://img.shields.io/crates/v/mediaway-test-media.svg" alt="crates.io"></a>
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack. Test-only helper crate — not for production use.

Deterministic Rust generators for test media (solid-color frames, silence), with a
BLAKE3-verified local cache. Mediaway never commits binary fixtures; tests call `ensure`
and the crate generates + verifies the file on first use.

## Quick start

```rust
use mediaway_test_media::ensure_solid_red_64x64;

let path = ensure_solid_red_64x64()?; // cached under local/.cache/test-media, BLAKE3-verified
```

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| BLAKE3-verified cache (`ensure`) | ✅ | Cache hit + matching hash → reuse; drift fails loudly |
| Solid RGBA8 frame (`red_64x64`) | ✅ | |
| Solid NV12 frame (`gray_nv12_64x64`) | ✅ | |
| Silent 48 kHz stereo PCM (`silence_48k_stereo_20ms`) | ✅ | |
| A/V sync sequences with known digests | 🛠️ | Planned |

## Docs

- [`docs/roadmap.md`](docs/roadmap.md) — stages, deferred items, design decisions
- Root [README](../../README.md) — workspace overview

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
