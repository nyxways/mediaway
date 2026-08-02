# mediaway-encoder-quicksync

Intel **vendor** backend for hardware encode (oneVPL / Quick Sync — direct `MFXInitEx`
session via [`vpl-sys`](../vpl-sys/), not routed through Media Foundation).

| Doc | Notes |
|-----|-------|
| [roadmap.md](docs/roadmap.md) | H.264/HEVC CPU-upload → Zero-Copy / AV1 (future) |
| [adr/](adr/) | Vendor-axis decisions |

Apps that want direct Quick Sync depend on this crate. Traits: `mediaway-encoder`. Not
wired into `mediaway-encoder`'s `auto` backend-selection logic yet.

**Hardware-verified on a real Intel UHD 770** — H.264 and HEVC encode both produce genuine
Annex-B bitstreams. AV1 encode is a confirmed ❌ on this iGPU generation. See
[ADR-0001](adr/0001-onevpl-quicksync-encode-surface.md).
