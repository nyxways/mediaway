# iso-bmff

<p align="center">
  <a href="https://docs.rs/iso-bmff"><img src="https://img.shields.io/docsrs/iso-bmff" alt="docs.rs"></a>
  <a href="https://crates.io/crates/iso-bmff"><img src="https://img.shields.io/crates/v/iso-bmff.svg" alt="crates.io"></a>
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production; public APIs may
> change without notice. Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack.

Sans-IO MP4 / ISOBMFF mux and demux (ISO/IEC 14496-12). A pure state machine: feed it
tracks and samples, collect container bytes — no files, sockets, or threads. Supports
fragmented MP4 (fMP4) output, unfragmented `stbl`-based demux, edit lists (`elst`), and
ClearKey CENC sample decryption (via `iso-cenc`).

## Quick start

```rust
use iso_bmff::mux::{Live, Muxer};
use iso_bmff::types::{Bytes, Codec, Rational, Sample, Track};

let mut muxer = Muxer::new();
muxer.add_track(Track {
    id: 0,
    codec: Codec::H264,
    time_base: Rational::new(1, 30),
    width: 1920,
    height: 1080,
    extra_data: Bytes::new(),
})?;

let mut muxer: Muxer<Live> = muxer.begin(); // Open -> Live
muxer.push_packet(&Sample {
    stream_id: 0, pts: 0, dts: 0, duration: 30,
    is_keyframe: true, is_discard: false,
    payload: Bytes::new(),
})?;
muxer.flush();

let mut mp4 = Vec::new();
muxer.poll_bytes(&mut mp4);

// Demux: feed bytes, poll packets — fully streaming.
let mut demuxer = iso_bmff::demux::Demuxer::new();
demuxer.push_bytes(&mp4);
while let Some(sample) = demuxer.poll_packet() { /* … */ }
```

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| Mux (typestate `Open` → `Live`, `ftyp`/`moov`/`moof`/`mdat`) | ✅ | Sample durations derived from `dts` deltas; fragmented batches |
| Demux (fMP4 + unfragmented `stbl`, mdat-before-moov) | ✅ | |
| Edit lists (`edts`/`elst`), discard / negative first PTS | ✅ | |
| ClearKey CENC (`tenc`/`senc`) | ✅ | Via `iso-cenc` |
| Sample entries | ✅ | H.264 (`avc1`), HEVC (`hvc1`/`hvcC`), AV1 (`av01`/`av1C`), VP9 (`vp09`/`vpcC`) |
| More codecs / sample-entry variants (`hev1`, Dolby Vision, …) | 🛠️ | As needed |

## Docs

- [`docs/roadmap.md`](docs/roadmap.md) — stages, deferred items, design decisions
- [`mediaway-container`](../mediaway-container/) — Mediaway-typed `mp4` surface over this crate
- Root [README](../../README.md) — container support matrix

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
