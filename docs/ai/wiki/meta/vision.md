# Vision (wiki pointer)

Canonical: [`docs/spec/vision.md`](../../../spec/vision.md).

Engineering context → pillars → license boundary (not a pillar).

| Pillar | One-liner |
|--------|-----------|
| Zero-Copy & HW | OS codecs + `GpuBufferHandle` **or** shared CPU buffers; copies explicit |
| Sans-IO cores | Mux/demux/bitstream without file/socket in core |
| High → low | Low-level traits/handles stay public |
| Honest costs | Named copy/readback/SW paths + rustdoc |

| Boundary | One-liner |
|----------|-----------|
| License / deps | MIT OR Apache-2.0; no libav*/GPL in Cargo graph; system ffmpeg oracle only |
| Status | Early / pre-1.0 — APIs may change often; not for production ([status](status.md)) |
| Maturity | Evidence for claimed scopes ([maturity-bar](maturity-bar.md)) |
