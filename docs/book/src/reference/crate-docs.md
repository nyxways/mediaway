# Crate Docs (docs.rs)

Per-crate API reference lives on docs.rs. Every published Mediaway crate links there
from its crates.io page; this page is the index.

## Published

| Crate | docs.rs | What it is |
|-------|---------|------------|
| `mediaway-common` | [docs.rs](https://docs.rs/mediaway-common) | Shared types (`Rational`, formats, `GpuBufferHandle`, packets/frames) |
| `iso-bmff` | [docs.rs](https://docs.rs/iso-bmff) | MP4 / ISOBMFF mux + demux core |
| `iso-cenc` | [docs.rs](https://docs.rs/iso-cenc) | ClearKey CENC sample crypto |
| `ebml-webm` | [docs.rs](https://docs.rs/ebml-webm) | EBML / WebM mux + demux core |
| `riff-wave-core` | [docs.rs](https://docs.rs/riff-wave-core) | WAV / RIFF PCM core |
| `adts-core` | [docs.rs](https://docs.rs/adts-core) | ADTS (raw AAC) core |
| `mpeg-audio` | [docs.rs](https://docs.rs/mpeg-audio) | MP3 (Layer III) core |
| `ogg-core` | [docs.rs](https://docs.rs/ogg-core) | Ogg page/packet core |
| `flv-core` | [docs.rs](https://docs.rs/flv-core) | FLV tag core |
| `mpeg-ts-core` | [docs.rs](https://docs.rs/mpeg-ts-core) | MPEG-2 Transport Stream core |
| `mediaway-container` | [docs.rs](https://docs.rs/mediaway-container) | Container facade (typed `mp4`/`webm`/`wav`/`adts`/`mp3`/`ogg`/`flv`/`ts`) |
| `mediaway-encoder` | [docs.rs](https://docs.rs/mediaway-encoder) | Encode traits + backends |
| `mediaway-decoder` | [docs.rs](https://docs.rs/mediaway-decoder) | Decode traits + backends |
| `mediaway-device` | [docs.rs](https://docs.rs/mediaway-device) | Capture/playback traits + backends |
| `mediaway` | [docs.rs](https://docs.rs/mediaway) | Convenience pipeline (`EncodeSession`, `platform`) |
| `mediaway-sw` | [docs.rs](https://docs.rs/mediaway-sw) | Pure Rust software codecs |
| `vpl-sys` | [docs.rs](https://docs.rs/vpl-sys) | oneVPL FFI bindings |
| `mediaway-test-media` | [docs.rs](https://docs.rs/mediaway-test-media) | Generated test fixtures (test-only) |
| `mediaway-avcli` | [docs.rs](https://docs.rs/mediaway-avcli) | AV CLI (mux) |
| `mediaway-avprobe` | [docs.rs](https://docs.rs/mediaway-avprobe) | Media probe CLI |

## Not yet published

These crates are not on crates.io (their `publish` flag is off while they stabilize) —
use the repository copies:

| Crate | Repo README |
|-------|-------------|
| `rtmp` | [`crates/rtmp/README.md`](https://github.com/nyxways/mediaway/blob/main/crates/rtmp/README.md) |
| `mediaway-ffi` | [`crates/mediaway-ffi/README.md`](https://github.com/nyxways/mediaway/blob/main/crates/mediaway-ffi/README.md) |
| `iso-bmff-wasm` | [`crates/iso-bmff-wasm/README.md`](https://github.com/nyxways/mediaway/blob/main/crates/iso-bmff-wasm/README.md) |

## How the support matrices stay in sync

The support-matrix pages ([Codec Support](./codec-support.md), [Container
Support](./container-support.md), [Device](./device-capture.md)) and the [Crate
Map](./crates.md) are `{{#include}}`d from the root [`README.md`](https://github.com/nyxways/mediaway/blob/main/README.md)
anchor blocks (`<!-- ANCHOR: … -->`). To update a matrix, edit the README — never the
book page. See [Extending the Book](../project/extending.md).
