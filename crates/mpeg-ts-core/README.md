# mpeg-ts-core

<p align="center">
  <a href="https://docs.rs/mpeg-ts-core"><img src="https://img.shields.io/docsrs/mpeg-ts-core" alt="docs.rs"></a>
  <a href="https://crates.io/crates/mpeg-ts-core"><img src="https://img.shields.io/crates/v/mpeg-ts-core.svg" alt="crates.io"></a>
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production; public APIs may
> change without notice. Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack.

Sans-IO MPEG-2 Transport Stream (ISO/IEC 13818-1) mux and demux for single-program
streams: PAT/PMT with correct PSI CRC-32, 188-byte TS packetization with adaptation-field
stuffing, and PES encapsulation with 33-bit PTS/DTS. Demux is an incremental reader that
tracks PAT/PMT and reassembles PES per PID. No I/O in the core.

## Quick start

```rust
use mpeg_ts_core::{Demuxer, ElementaryStream, Muxer, StreamType};

let streams = &[ElementaryStream { pid: 0x100, stream_type: StreamType::H264 }];
let mut muxer = Muxer::new(1, 0x101, streams)?;

let mut ts = Vec::new();
muxer.write_pat_pmt(&mut ts);
muxer.write_access_unit(0x100, &access_unit, 90_000, Some(90_000), true, &mut ts)?;

let mut demuxer = Demuxer::new();
demuxer.push_bytes(&ts);
while let Some(au) = demuxer.poll_access_unit()? { /* PTS/DTS + payload per PID */ }
```

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| Mux (PAT/PMT + per-PID PES, 188-byte packets) | ✅ | Correct adaptation-field stuffing (real bug found via round-trip tests) |
| Demux (incremental, PAT/PMT tracking, per-PID PES reassembly) | ✅ | |
| Timestamps | ✅ | PTS-only or PTS+DTS, 33-bit bit-packed |
| Elementary streams | ✅ | H.264, HEVC, AAC, MP3 |
| Multi-program transport streams | 🛠️ | One PAT entry only |
| PCR insertion/extraction | 🛠️ | `PCR_PID` written as unassigned |
| Multi-packet PSI section reassembly | 🛠️ | PAT/PMT must fit one TS packet |

## Docs

- [`docs/roadmap.md`](docs/roadmap.md) — stages, deferred items, design decisions
- [`mediaway-container`](../mediaway-container/) — Mediaway-typed `ts` surface over this crate
- Root [README](../../README.md) — container support matrix

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
