# Platform development order

Canonical workspace index: [`docs/roadmap.md`](../../../roadmap.md).
Per-crate plans: `crates/<name>/docs/roadmap.md` (and `tools/…`).

| Order | Platform | Notes |
|-------|----------|-------|
| 1 | Windows | WMF, DX11 Zero-Copy — first MVP |
| 2 | Web | WebCodecs, WebGPU |
| 3 | Linux | VA-API / Vulkan Video |
| 4+ | Other | Apple VideoToolbox, Android MediaCodec, … |

Do not start later-platform backends before the previous platform has a working encode/mux path (decode as needed for tests).

