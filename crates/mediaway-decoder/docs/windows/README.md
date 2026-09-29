# mediaway-decoder-windows

Windows **platform backend** for hardware video decode (Media Foundation, DX11 Zero-Copy output).

| Doc | Notes |
|-----|-------|
| [roadmap.md](docs/roadmap.md) | WMF H.264 DX11 decode |
| [adr/](adr/) | OS-specific decisions |

Apps that want WMF decode depend on this crate directly. Traits: `mediaway-decoder`.

## Status

- **H.264 CPU decode** — real (in-box SW MFT, no GPU required), verified encode→decode round trip.
- **HEVC / VP9 CPU decode** — hardware-verified and wired into `WindowsVideoDecoder::open`. HEVC read back out of an MP4 (`hvcC` + length-prefixed samples) decodes: `resolve_framing` converts it to Annex-B (2026-09-21).
- **AV1 CPU decode** — 🛠️ planned: the driver only offers AYUV output, not NV12.
- **Opus decode** — hardware-verified; implements the `AudioDecoder` trait.
- **AAC decode** — `WmfAacDecoder` over the inbox `CMSAACDecMFT`, hardware-verified sample-exact round trip; needs the stream's `AudioSpecificConfig` at open ([ADR-0006](../../adr/windows/0006-wmf-aac-decode.md)).
- **D3D12 native decode** — H.264 implemented (45 unit tests) but paused: the hardware-gated test triggers a real `DXGI_ERROR_DEVICE_HUNG` GPU-driver TDR after 3 real bugs were already found and fixed. HEVC/AV1 decode are on hold pending that. See [ADR-0002](adr/0002-d3d12-native-video-decode.md).
