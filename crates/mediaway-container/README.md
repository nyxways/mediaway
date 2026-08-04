# mediaway-container

<p align="center">
  <a href="https://docs.rs/mediaway-container"><img src="https://img.shields.io/docsrs/mediaway-container" alt="docs.rs"></a>
  <a href="https://crates.io/crates/mediaway-container"><img src="https://img.shields.io/crates/v/mediaway-container.svg" alt="crates.io"></a>
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production; public APIs may
> change without notice. Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack.

The Mediaway container facade: shared mux/demux traits plus Mediaway-typed modules for
eight container formats — `mp4`, `webm`, `wav`, `adts`, `mp3`, `ogg`, `flv`, `ts`. Each
module is a sans-io state machine over the workspace's format cores, speaking
`mediaway_common`'s `StreamInfo`/`Packet` types so apps can swap formats behind one
interface.

## Quick start

```rust
use mediaway_common::{Bytes, CodecKind, Packet, Rational, StreamInfo, VideoGeometry};
use mediaway_container::mp4;

// Typestate mux: register tracks, begin, push packets, collect bytes.
let mut muxer = mp4::Muxer::new();
let v_track = muxer.add_track(StreamInfo::Video {
    id: 0, codec: CodecKind::H264, time_base: Rational::new(1, 30),
    geometry: VideoGeometry { width: 1920, height: 1080 },
    extra_data: Bytes::new(),
})?;

let mut muxer = muxer.begin(); // Open -> Live
muxer.push_packet(&Packet {
    stream_id: v_track, pts: 0, dts: 0, duration: 1,
    is_keyframe: true, is_discard: false,
    payload: Bytes::from_static(b"\x00\x00\x00\x01"),
})?;
muxer.flush();

let mut mp4_bytes = Vec::new();
muxer.poll_bytes(&mut mp4_bytes);

// Streaming demux: feed bytes, poll packets.
let mut demux = mp4::Demuxer::new();
demux.push_bytes(&mp4_bytes);
while let Some(pkt) = demux.poll_packet() { /* … */ }
```

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Format | Module | Status | Notes |
| ------ | ------ | ------ | ----- |
| MP4 / fMP4 | `mp4` | ✅ | Mux + demux + ClearKey CENC |
| WebM | `webm` | ✅ | Mux + demux (full Matroska-profile demux) |
| WAV / RIFF (PCM) | `wav` | ✅ | Mux + demux; own method shape (size known up front) |
| ADTS (raw AAC) | `adts` | ✅ | `Mux`/`Demux` traits |
| MP3 | `mp3` | ✅ | `Demux` only; no `Mux` fit (per-frame padding bit) |
| Ogg | `ogg` | ✅ | `Mux`/`Demux`; codec from `OpusHead`/Vorbis id header |
| FLV | `flv` | ✅ | Codec-aware mux + demux (AVC video, AAC/MP3 audio) |
| MPEG-TS | `ts` | ✅ | `Demux` only; mux exposes per-PID `write_access_unit` |
| VP8 `CodecKind` mapping | — | ❌ | Recognized structurally in WebM, not representable yet |

## Docs

- [`docs/roadmap.md`](docs/roadmap.md) — stages, deferred items, design decisions
- Sans-IO cores: [`iso-bmff`](../iso-bmff/), [`ebml-webm`](../ebml-webm/), [`riff-wave-core`](../riff-wave-core/), [`adts-core`](../adts-core/), [`mpeg-audio`](../mpeg-audio/), [`ogg-core`](../ogg-core/), [`flv-core`](../flv-core/), [`mpeg-ts-core`](../mpeg-ts-core/)
- Root [README](../../README.md) — container support matrix

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
