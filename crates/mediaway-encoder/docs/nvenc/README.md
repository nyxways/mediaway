# mediaway-encoder-nvenc

NVIDIA **vendor** backend for hardware encode (direct `nvEncodeAPI64.dll` — not routed
through Media Foundation).

| Doc | Notes |
|-----|-------|
| [roadmap.md](docs/roadmap.md) | H.264 CPU-upload (Windows/D3D11) → Zero-Copy / multi-codec (future) |
| [adr/](adr/) | Vendor-axis decisions |

Apps that want direct NVENC depend on this crate. Traits: `mediaway-encoder`. Not wired into
`mediaway-encoder`'s `auto` backend-selection logic yet — a later integration pass.

**Hardware-verified 2026-07-29 on a real NVIDIA GeForce RTX 4090** (driver 32.0.15.9579) —
see [ADR-0001](adr/0001-nvenc-vendor-backend.md) 2026-07-29 addendum for what was built
and confirmed, including a real bug found (and worked around) in the `nvenc` crate's native
CPU input-buffer lock path.
