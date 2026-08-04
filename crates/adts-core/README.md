# adts-core

<p align="center">
  <a href="https://docs.rs/adts-core"><img src="https://img.shields.io/docsrs/adts-core" alt="docs.rs"></a>
  <a href="https://crates.io/crates/adts-core"><img src="https://img.shields.io/crates/v/adts-core.svg" alt="crates.io"></a>
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production; public APIs may
> change without notice. Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack.

Sans-IO ADTS framing for raw AAC elementary streams — the common file-less AAC transport
(a 7-byte header per frame, no container-level header). Mux appends one frame per call;
demux is a true incremental `push_bytes`/`poll_frame` reader that handles partial input.
No I/O in the core.

## Quick start

```rust
use adts_core::{AdtsConfig, AacProfile, Demuxer, Muxer};

let config = AdtsConfig { profile: AacProfile::Lc, sample_rate: 48_000, channels: 2 };
let muxer = Muxer::new(config)?;

let mut adts = Vec::new();
muxer.write_frame(&raw_aac_frame, &mut adts)?; // one 7-byte-header ADTS frame

let mut demuxer = Demuxer::new();
demuxer.push_bytes(&adts);
while let Some(aac) = demuxer.poll_frame()? { /* one raw AAC frame */ }
```

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| Mux (no-CRC 7-byte header) | ✅ | One raw-data-block per frame (the common AAC-LC case) |
| Demux (incremental, partial-input safe) | ✅ | Reads both no-CRC (7-byte) and CRC (9-byte) headers |
| Mux-side CRC header | 🛠️ | Demux already reads it; mux writes no-CRC only |
| Multi-raw-data-block frames exposed | 🛠️ | Payload bytes survive; internal AAC-frame boundary not exposed |

## Docs

- [`docs/roadmap.md`](docs/roadmap.md) — stages, deferred items, design decisions
- [`mediaway-container`](../mediaway-container/) — Mediaway-typed `adts` surface over this crate
- Root [README](../../README.md) — container support matrix

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
