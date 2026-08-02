# riff-wave

Sans-IO RIFF/WAVE (PCM) mux + demux. Freestanding — no Mediaway types.

Unprefixed reusable core — naming [ADR-0012](../../docs/adr/0012-unprefixed-reusable-cores.md).

v1 scope: PCM integer + IEEE float `fmt ` only; no `WAVE_FORMAT_EXTENSIBLE`, no
compressed WAV payloads. See [`docs/roadmap.md`](docs/roadmap.md) and
[`adr/0001`](adr/0001-riff-wave-freestanding-core.md).
