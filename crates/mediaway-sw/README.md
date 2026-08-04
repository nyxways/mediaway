# mediaway-sw

<p align="center">
  <a href="https://docs.rs/mediaway-sw"><img src="https://img.shields.io/docsrs/mediaway-sw" alt="docs.rs"></a>
  <a href="https://crates.io/crates/mediaway-sw"><img src="https://img.shields.io/crates/v/mediaway-sw.svg" alt="crates.io"></a>
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production; public APIs may
> change without notice. Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack.

Pure-Rust software codecs — no C codec FFI, no GPL/LGPL in the graph. Sans-IO adapters
over Rust cores: an H.264 decoder, AV1 encode via `rav1e`, Opus encode/decode via
`unsafe-libopus`, raw PCM passthrough, and audio processing (AEC3 + NS + AGC2, RNN VAD)
via `sonora`. Opt-in by design — a fallback, never a silent default.

## Quick start

```rust
use mediaway_common::{Rational, SampleFormat};
use mediaway_sw::pcm::{PcmFormat, PcmPassthroughConfig, PcmEncoder};

let config = PcmPassthroughConfig::new(
    PcmFormat { sample_format: SampleFormat::S16, sample_rate: 48_000, channels: 2 },
    Rational::new(1, 48_000),
);
let mut encoder = PcmEncoder::new(config);
encoder.push_frame(&pcm_audio_frame)?;
while let Some(packet) = encoder.poll_packet()? { /* raw PCM packets */ }
```

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| PCM / raw passthrough (encode + decode) | ✅ | Format/rate/channel validation, no re-encode |
| Opus encode/decode | ✅ | `unsafe-libopus`; ~20% CPU vs C reference (no asm/SIMD) |
| AV1 encode | ✅ | `rav1e` behind a sans-io adapter; 8-bit I420 input |
| H.264 decode | 🆗 | Baseline / CAVLC / I-slice pixel decoder — real bitstream-to-pixels, hand-verified |
| Audio processing (AEC3 + NS + AGC2) + RNN VAD | ✅ | `apm` module; SIMD-accelerated |
| H.264: CABAC, `I_NxN`, P/B slices, deblocking | 🛠️ | Cleanly rejected today, not mishandled |
| H.264 encode | 🛠️ | Planned |

## Docs

- [`docs/roadmap.md`](docs/roadmap.md) — stages, deferred items, design decisions
- [`mediaway-encoder`](../mediaway-encoder/) — software path wired as the AV1 fallback
- Root [README](../../README.md) — CPU/SW codec support table

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
