# Wiki index

Accumulate knowledge across sessions so agents need not re-trace every system from scratch.
**After investigating a system or discovering a convention, add a page link under the category `index.md`.**

| Category | Summary |
|----------|---------|
| [pipeline](pipeline/index.md) | End-to-end flow · API layers (low-level first-class) |
| [common](common/index.md) | Shared types — `Rational`, formats, `GpuBufferHandle` |
| [encode](encode/index.md) | Encoder traits · backends · bitrate/presets |
| [decode](decode/index.md) | Decoders · HW decode · output handles |
| [container](container/index.md) | Muxer / Demuxer — sans-io cores; MP4, WebM, WAV, ADTS, MP3, Ogg, FLV, MPEG-TS |
| [zero-copy](zero-copy/index.md) | ⚡ marks · GPU handles · shared CPU · wgpu interop |
| [platform](platform/index.md) | OS/runtime APIs · support-matrix pointer · WMF/DX11 |
| [device](device/index.md) | Camera · mic · screen capture |
| [audio](audio/index.md) | Audio enhancement — AEC/NS/AGC2/VAD (`mediaway-audio-apm`) |
| [license](license/index.md) | Licenses · deny · allowed SW fallbacks |
| [meta](meta/index.md) | Workspace · hooks · ADR ops |

---

Layout: `docs/ai/wiki/<category>/<page>.md`; category TOC at `…/<category>/index.md`.
New category → add directory + `index.md` and a row in the table above.
**Every wiki markdown file (including indexes) is limited to 100 lines** — split into pages/sub-indexes when needed.
**English only.**
