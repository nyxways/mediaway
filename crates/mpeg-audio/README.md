# mpeg-audio

<p align="center">
  <a href="https://docs.rs/mpeg-audio"><img src="https://img.shields.io/docsrs/mpeg-audio" alt="docs.rs"></a>
  <a href="https://crates.io/crates/mpeg-audio"><img src="https://img.shields.io/crates/v/mpeg-audio.svg" alt="crates.io"></a>
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production; public APIs may
> change without notice. Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack.

Sans-IO MPEG-1/2/2.5 Layer III ("MP3") elementary-stream framing: mux writes a correct
4-byte header around already-encoded frame bodies, demux is an incremental
`push_bytes`/`poll_frame` reader. MPEG audio has no container-level header — frames
self-describe, so this crate frames, it does not encode/decode. No I/O in the core.

## Quick start

```rust
use mpeg_audio::{ChannelMode, Demuxer, FrameHeader, MpegVersion, Muxer};

let header = FrameHeader {
    version: MpegVersion::Mpeg1,
    bitrate_kbps: 128,
    sample_rate: 44_100,
    channel_mode: ChannelMode::Stereo,
};
let muxer = Muxer::new(header)?;

let mut mp3 = Vec::new();
muxer.write_frame(&encoded_body, true, &mut mp3)?; // per-call padding bit

let mut demuxer = Demuxer::new();
demuxer.push_bytes(&mp3);
while let Some(frame) = demuxer.poll_frame()? { /* one Layer III frame body */ }
```

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| Mux (Layer III header + per-call padding) | ✅ | MPEG-1/2/2.5 bitrate/sample-rate tables |
| Demux (incremental, partial-input safe) | ✅ | Reads no-CRC and CRC headers; frame length cross-checked against spec values |
| Layer I / II | ❌ | Rejected with a clear `UnsupportedLayer` error, not misparsed |
| Mux-side CRC | 🛠️ | Demux already reads it; mux writes no-CRC only |
| ID3v1/ID3v2 tag skip, Xing/VBR headers | 🛠️ | Assumes frame-aligned input |

## Docs

- [`docs/roadmap.md`](docs/roadmap.md) — stages, deferred items, design decisions
- [`mediaway-container`](../mediaway-container/) — Mediaway-typed `mp3` surface over this crate
- Root [README](../../README.md) — container support matrix

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
