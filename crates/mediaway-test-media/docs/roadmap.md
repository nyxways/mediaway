# mediaway-test-media — roadmap

Supports every platform stage with generated, BLAKE3-checked fixtures.  
Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).  
Convention: [`docs/conventions/testing.md`](../../../docs/conventions/testing.md).

## Stages

### 0 — Cache + hash

- [x] `ensure` + BLAKE3 verify
- [x] `ensure_solid_red_64x64`
- [x] More raw patterns (NV12, PCM) as encoders need them —
      `ensure_solid_gray_nv12_64x64`, `ensure_pcm_silence_48k_stereo_20ms`

### 1 — Windows encode tests

- [ ] Fixtures / generators for WMF input sizes and strides
- [ ] Optional tiny synthetic elementary streams (Pure Rust)

### 2 — Web / Linux

- [ ] Generators matching WebCodecs / VA-API test sizes

### 3 — A/V sync helpers

- [ ] Multi-frame sequences with known digests for round-trips
