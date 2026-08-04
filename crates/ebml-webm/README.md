# ebml-webm

<p align="center">
  <a href="https://docs.rs/ebml-webm"><img src="https://img.shields.io/docsrs/ebml-webm" alt="docs.rs"></a>
  <a href="https://crates.io/crates/ebml-webm"><img src="https://img.shields.io/crates/v/ebml-webm.svg" alt="crates.io"></a>
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production; public APIs may
> change without notice. Part of the [Mediaway](https://github.com/nyxways/mediaway)
> media stack.

Sans-IO EBML / WebM mux and demux. Demux handles the common Matroska/WebM profile —
`SimpleBlock` and `BlockGroup` frames, Xiph/fixed/EBML lacing, `Cues`/`SeekHead`
(index) — and mux writes spec-valid WebM with known-size clusters. Pure state machine:
push bytes in, poll frames out; no I/O in the core.

## Quick start

```rust
use ebml_webm::mux::{Live, Muxer};
use ebml_webm::types::{Bytes, TrackInfo};

let mut muxer = Muxer::new();
muxer.add_track(TrackInfo {
    track_number: 1,
    track_type: 1, // video
    codec_id: "V_VP9".to_owned(),
    codec_private: None,
    width: 1920,
    height: 1080,
    sample_rate: 0.0,
    channels: 0,
})?;

let mut muxer: Muxer<Live> = muxer.begin();
muxer.push_frame(1, 0, true, &vp9_keyframe)?; // track, timecode, keyframe, payload
muxer.flush();

let mut webm = Vec::new();
muxer.poll_bytes(&mut webm);

// Demux: feed bytes, poll frames — the index is exposed informationally.
let mut demuxer = ebml_webm::Demuxer::new();
demuxer.push_bytes(&webm);
while let Some(frame) = demuxer.poll_frame() { /* … */ }
let _index = demuxer.cues();
```

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| Demux: EBML VINT, `Segment`/`Tracks`/`Cluster`/`SimpleBlock` | ✅ | |
| Demux: lacing (Xiph, fixed, EBML), `BlockGroup`/`BlockDuration` | ✅ | |
| Demux: `Cues` / `SeekHead` index | ✅ | Exposed informationally; no seeking in the core |
| Mux (typestate `Open` → `Live`) | ✅ | Known-size clusters; `push_frame` + EBML-laced `push_laced_frames` |
| Indefinite-size `Cluster` lookahead (live streams) | ✅ | Sibling-ID lookahead bounds the open-element stack |
| Mux-side lacing | ✅ | EBML lacing writer (`push_laced_frames`) |
| ffprobe oracle on mux output | ✅ | `tests/mux_oracle.rs`, Tier 7 (skips if ffprobe absent) |

## Docs

- [`docs/roadmap.md`](docs/roadmap.md) — stages, deferred items, design decisions
- [`mediaway-container`](../mediaway-container/) — Mediaway-typed `webm` surface over this crate
- Root [README](../../README.md) — container support matrix

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
