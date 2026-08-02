# Pipeline overview

```text
[device | demuxer] → packets → [decoder] → frames (Cpu | Gpu)
                                              ↓
                                         (app / compositor)
                                              ↓
[muxer] ← packets ← [encoder] ← frames (Cpu | Gpu)
```

- **Library API** = compose encode/decode/mux/demux/device crates.
- **Mux/demux/bitstream/config** = sans-io cores ([container/sans-io](../container/sans-io.md)); file/OPFS only in adapters.
- **Low-level first-class** ([api-layers](api-layers.md)) — traits/handles usable without convenience wrappers.
- **`mediaway-avcli`** is a separate tool — do not leak FFmpeg concepts into the public library API.
- Timebase: `Rational { num, den }` ([common/rational](../common/rational.md)).
- GPU Zero-Copy: `GpuBufferHandle` ([zero-copy/handles](../zero-copy/handles.md)).
- CPU Zero-Copy: shared PCM / `Bytes` (same README **⚡** — [marks](../zero-copy/marks.md)).

MVP order (platforms): **Windows → Web → Linux → other**.
Per-crate stages: each crate’s `docs/roadmap.md`.
Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).
