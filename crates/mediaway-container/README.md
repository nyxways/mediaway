# mediaway-container

Facade for mux/demux: `ContainerFormat`, `Mux`, `Demux`, `DemuxDecrypt`, plus Mediaway-typed MP4.

| Crate | Role |
|-------|------|
| `iso-bmff` | Freestanding ISOBMFF/MP4 (no Mediaway types) |
| `mediaway-container` | Traits + `mp4` (+ `mp4_parser`) over `iso-bmff` |
| `iso-cenc` | Unprefixed ClearKey CENC |

Apps and CLIs depend on this crate (or on `iso-bmff` when Mediaway types are not needed).

## Status

MP4 (`iso-bmff`, fragmented mux only) and WebM (`ebml-webm`, full Matroska-profile demux + mux) are real, tested, and wired in — including ClearKey CENC. VP8 video still has no `CodecKind` mapping.

The six audio/general container crates (`riff-wave`/WAV, `adts`, `mpeg-audio`/MP3, `ogg`, `flv`, `mpeg-ts`) are wired in too, but several don't fit the shared `Mux`/`Demux` trait shape honestly (e.g. RIFF needs a known-upfront size, MP3 needs an explicit padding bit, MPEG-TS uses a fixed 90 kHz clock) and expose their own method shapes instead — see [`adr/0002`](adr/0002-audio-and-general-container-facades.md) and each crate's own README.

No non-WebM Matroska mux, or any other container format, is planned beyond what's listed in the root [README § Container support](../../README.md#container-support).

Packaging: ADR-0003 · naming v1: ADR-0012.
