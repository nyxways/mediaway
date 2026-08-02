# Container scaffold

| Crate | Path | Role |
|-------|------|------|
| `iso-bmff` | `crates/iso-bmff` | Freestanding ISOBMFF/MP4 mux+demux |
| `ebml-webm` | `crates/ebml-webm` | Freestanding EBML/WebM demux (mux deferred) |
| `riff-wave` | `crates/riff-wave` | Freestanding WAV/PCM mux+demux |
| `adts` | `crates/adts` | Freestanding raw-AAC elementary stream mux+demux |
| `mpeg-audio` | `crates/mpeg-audio` | Freestanding MP3/Layer III mux+demux |
| `ogg` | `crates/ogg` | Freestanding Ogg page/packet mux+demux |
| `flv` | `crates/flv` | Freestanding FLV tag mux+demux |
| `mpeg-ts` | `crates/mpeg-ts` | Freestanding MPEG-2 TS mux+demux |
| `mediaway-container` | `crates/mediaway-container` | Traits + Mediaway `mp4`/`webm`/`adts`/`wav`/`mp3`/`ogg`/`flv`/`ts` |

Product callers: `mediaway_container::{mp4, webm, adts, wav, mp3, ogg, flv, ts}`.
Freestanding / no Mediaway types: `iso_bmff`, `ebml_webm`, `riff_wave`,
`adts`, `mpeg_audio`, `ogg`, `flv`, `mpeg_ts`.
