# mediaway — docs

Convenience **pipeline facade**: composes `mediaway-encoder` + `mediaway-container`
(+ `mediaway-device` for capture) into [`EncodeSession`] and platform auto-dispatch,
so apps don't hand-roll the encoder→muxer poll loop. Low-level traits stay fully
public and reachable without this crate — see `examples/container/mux_demux_mp4.rs`.

Design decision: workspace [ADR-0014](../../docs/adr/0014-pipeline-convenience-crate.md).

| Doc | Notes |
|-----|-------|
| [roadmap.md](docs/roadmap.md) | Stages |
| [adr/](adr/) | Crate-local decisions (workspace ADR-0014 covers the crate's existence) |
