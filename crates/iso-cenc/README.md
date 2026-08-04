# iso-cenc

<p align="center">
  <a href="https://docs.rs/iso-cenc"><img src="https://img.shields.io/docsrs/iso-cenc" alt="docs.rs"></a>
  <a href="https://crates.io/crates/iso-cenc"><img src="https://img.shields.io/crates/v/iso-cenc.svg" alt="crates.io"></a>
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production; public APIs may
> change without notice. Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack.

Sans-IO sample encryption for ISO Common Encryption (ISO/IEC 23001-7): the AES-128-CTR
`cenc` scheme with subsample-aware crypto. Callers supply the keys — this is sample
crypto, not a DRM client (no CDM, no license acquisition).

## Quick start

```rust
use iso_cenc::{decrypt_cenc, Pattern, Subsample};

let key = [0u8; 16];
let iv = [0u8; 16]; // 16-byte CTR init block
let mut sample = [0u8; 64];
let subsamples = &[Subsample { clear_bytes: 16, protected_bytes: 48 }];

decrypt_cenc(&key, &iv, Pattern::NONE, &mut sample, subsamples)?;
```

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| `cenc` AES-128-CTR, subsample-aware | ✅ | Clear ranges do not advance the CTR counter |
| Used by `iso-bmff` demux (`tenc` / `senc`) | ✅ | |
| `cens` / `cbc1` / `cbcs` schemes | 🛠️ | When a concrete product need appears |

## Docs

- [`docs/roadmap.md`](docs/roadmap.md) — stages, deferred items, design decisions
- [`iso-bmff`](../iso-bmff/) — MP4 mux/demux with ClearKey wiring
- Root [README](../../README.md) — container support matrix

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
