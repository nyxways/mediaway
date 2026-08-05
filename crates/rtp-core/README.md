# rtp-core

<p align="center">
  <a href="https://docs.rs/rtp-core"><img src="https://img.shields.io/docsrs/rtp-core" alt="docs.rs"></a>
  <a href="https://crates.io/crates/rtp-core"><img src="https://img.shields.io/crates/v/rtp-core.svg" alt="crates.io"></a>
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production; public APIs may
> change without notice. Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack.

Sans-IO RTP payloadization for H.264 and HEVC video: the RTP fixed header (RFC 3550
§5.1) plus single-NAL-unit and FU-A/FU fragmentation packet formats (RFC 6184, RFC
7798). Packetize turns one NAL unit into one or more `RtpPacket`s under a
caller-supplied per-packet payload budget; depacketize reassembles them back into NAL
units from in-order-arriving packet payloads. No I/O in the core — no socket, no RTCP,
no SRTP.

## Quick start

```rust
use rtp_core::{h264, RTP_VIDEO_CLOCK_RATE_HZ};

let mut packetizer = h264::Packetizer::new(1400, 96, 0x1122_3344, 1)?;
let packets = packetizer.packetize(&nal_unit, 90_000, /* marker */ true)?;

let mut wire = Vec::new();
for packet in &packets {
    packet.write(&mut wire)?; // send `wire` over your own socket, then clear it
    wire.clear();
}

let mut depacketizer = h264::Depacketizer::new();
if let Some(nal) = depacketizer.depacketize(&packets[0].payload)? {
    // one reassembled NAL unit
}
```

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| RTP fixed header (12-byte, no CSRC/extension/padding) | ✅ | Build + parse, round-trip tested |
| H.264 single-NAL + FU-A packetize/depacketize | ✅ | RFC 6184 §5.6/§5.8 |
| HEVC single-NAL + FU packetize/depacketize | ✅ | RFC 7798 §4.4.1/§4.4.3 |
| Aggregation packets (STAP-A/STAP-B/MTAP, AP) | 🛠️ | Named scope cut — see `docs/roadmap.md` |
| RTCP / SRTP | 🛠️ | Separate, larger scope |
| Out-of-order / loss-tolerant depacketize | 🛠️ | Current depacketize assumes in-order arrival |

## Docs

- [`docs/roadmap.md`](docs/roadmap.md) — stages, deferred items, design decisions
- Root [README](../../README.md) — codec/container support matrix

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
