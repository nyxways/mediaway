# riff-wave-core

<p align="center">
  <a href="https://docs.rs/riff-wave-core"><img src="https://img.shields.io/docsrs/riff-wave-core" alt="docs.rs"></a>
  <a href="https://crates.io/crates/riff-wave-core"><img src="https://img.shields.io/crates/v/riff-wave-core.svg" alt="crates.io"></a>
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production; public APIs may
> change without notice. Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack.

Sans-IO RIFF/WAVE mux and demux for PCM audio (integer and IEEE float). Mux buffers
pushed samples and writes a complete `.wav` on `finish()` (RIFF chunk sizes must be
known up front); demux parses a complete buffer back into a format description and raw
samples. No I/O in the core.

## Quick start

```rust
use riff_wave_core::{Muxer, parse, SampleFormat, WaveFormat};

let format = WaveFormat {
    sample_format: SampleFormat::Pcm,
    channels: 2,
    sample_rate: 48_000,
    bits_per_sample: 16,
};

let mut muxer = Muxer::new(format);
muxer.push_samples(&pcm_bytes);
let wav = muxer.finish(); // complete RIFF/WAVE file

// Parse it back: format description + raw sample payload.
let (parsed, samples) = parse(&wav)?;
assert_eq!(parsed.sample_rate, 48_000);
```

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| Mux (PCM integer + IEEE float, `fmt `/`data` chunks) | ✅ | |
| Demux (`parse`, skips unknown chunks like `LIST`/`fact`) | ✅ | Complete-buffer API (not incremental) |
| `WAVE_FORMAT_EXTENSIBLE` (multichannel masks) | 🛠️ | |
| Compressed WAV payloads (ADPCM, µ-law/A-law, MP3-in-WAV) | 🛠️ | Needs a per-format decode step outside container scope |
| RF64 / W64 (> 4 GiB files) | 🛠️ | Classic 32-bit RIFF size field only |

## Docs

- [`docs/roadmap.md`](docs/roadmap.md) — stages, deferred items, design decisions
- [`mediaway-container`](../mediaway-container/) — Mediaway-typed `wav` surface over this crate
- Root [README](../../README.md) — container support matrix

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
