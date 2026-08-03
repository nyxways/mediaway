# Container scaffold

| Crate | Path | Role |
|-------|------|------|
| `iso-bmff` | `crates/iso-bmff` | Freestanding ISOBMFF/MP4 mux+demux |
| `ebml-webm` | `crates/ebml-webm` | Freestanding EBML/WebM demux (mux deferred) |
| `riff-wave-core` | `crates/riff-wave-core` | Freestanding WAV/PCM mux+demux |
| `adts-core` | `crates/adts-core` | Freestanding raw-AAC elementary stream mux+demux |
| `mpeg-audio` | `crates/mpeg-audio` | Freestanding MP3/Layer III mux+demux |
| `ogg-core` | `crates/ogg-core` | Freestanding Ogg page/packet mux+demux |
| `flv-core` | `crates/flv-core` | Freestanding FLV tag mux+demux |
| `mpeg-ts-core` | `crates/mpeg-ts-core` | Freestanding MPEG-2 TS mux+demux |
| `mediaway-container` | `crates/mediaway-container` | Traits + Mediaway `mp4`/`webm`/`adts-core`/`wav`/`mp3`/`ogg-core`/`flv-core`/`ts` |

Product callers: `mediaway_container::{mp4, webm, adts-core, wav, mp3, ogg-core, flv-core, ts}`.
Freestanding / no Mediaway types: `iso_bmff`, `ebml_webm`, `riff_wave`,
`adts-core`, `mpeg_audio`, `ogg-core`, `flv-core`, `mpeg_ts`.
