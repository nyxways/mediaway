# mediaway-encoder-windows

Windows **platform backend** for hardware encode (Media Foundation, DX11 Zero-Copy).

| Doc | Notes |
|-----|-------|
| [roadmap.md](docs/roadmap.md) | WMF H.264 → DX11 → AAC |
| [adr/](adr/) | OS-specific decisions |

Apps that want WMF depend on this crate directly (or a future `mediaway-encoder` `windows` feature). Traits: `mediaway-encoder`.

## Status

- **H.264 encode ⚡** — a DX11 NV12 surface → HW MFT; other codecs and decode are 🆗 prototypes on the same WMF path.
- **Opus encode** — ❌, no inbox MFT exists.
- **D3D12 native encode** — H.264 and HEVC 🆗, **hardware-verified** on an RTX 4090 (`ID3D12VideoDevice3`/`ID3D12VideoEncoder`, distinct from feeding D3D12 textures into WMF) — several real driver gotchas found only by running on hardware. AV1 encode was probed for real (driver reports `IsSupported == true` at the capability-query level but `CODEC_NOT_SUPPORTED` for every actual configuration); the encoder itself is **not implemented** — scoped as a follow-up. See [ADR-0007](adr/0007-d3d12-native-video-encode.md).
