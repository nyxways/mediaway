# ogg-core

<p align="center">
  <a href="https://docs.rs/ogg-core"><img src="https://img.shields.io/docsrs/ogg-core" alt="docs.rs"></a>
  <a href="https://crates.io/crates/ogg-core"><img src="https://img.shields.io/crates/v/ogg-core.svg" alt="crates.io"></a>
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production; public APIs may
> change without notice. Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack.

Sans-IO Ogg page/packet framing (RFC 3533): mux writes spec-valid pages (one packet per
page), demux is a general incremental reader that handles multi-packet pages, packets
spanning continuation pages, and CRC verification. The transport for Opus, Vorbis, and
FLAC logical bitstreams. No I/O in the core.

## Quick start

```rust
use ogg_core::{Demuxer, Muxer};

let mut muxer = Muxer::new(serial_number);
let mut ogg = Vec::new();
muxer.push_packet(&opus_head, 0, false, &mut ogg)?; // bos set automatically
muxer.push_packet(&opus_packet, 44100, true, &mut ogg)?; // eos on the last page

let mut demuxer = Demuxer::new();
demuxer.push_bytes(&ogg);
while let Some(packet) = demuxer.poll_packet()? {
    // packet.data (continuation pages merged), packet.granule_position, …
}
```

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| Mux (one packet per page, correct lacing + CRC, bos/eos) | ✅ | |
| Demux (multi-packet pages, cross-page continuation, CRC) | ✅ | General case, tested against hand-built pages |
| Mux: multi-packet page batching | 🛠️ | Real encoders pack small packets; this mux emits one packet per page |
| Mux: packets over 65024 bytes (continuation-page split) | 🛠️ | `PacketTooLargeForSinglePage` today |
| Multi-logical-stream interleaving / chaining | 🛠️ | One muxer = one `serial` |

## Docs

- [`docs/roadmap.md`](docs/roadmap.md) — stages, deferred items, design decisions
- [`mediaway-container`](../mediaway-container/) — Mediaway-typed `ogg` surface over this crate
- Root [README](../../README.md) — container support matrix

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
