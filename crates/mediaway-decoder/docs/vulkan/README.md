# mediaway-decoder-vulkan

Cross-platform **Vulkan Video** decode backend (`VK_KHR_video_decode_queue` +
`VK_KHR_video_decode_h264`/`_h265`/`_av1`), bound via
[`vulkanalia`](https://crates.io/crates/vulkanalia) (same binding crate
already adopted by the sibling `mediaway-encoder-vulkan`).

| Doc | Notes |
|-----|-------|
| [roadmap.md](docs/roadmap.md) | Stage-by-stage plan: probe → session → per-codec decode → Zero-Copy |
| [adr/](adr/) | Bindings survey, crate placement, general-GOP scope |

Unlike `mediaway-decoder-windows` / `mediaway-decoder-linux`, this crate is
**not** OS-suffixed: Vulkan Video is one portable API reachable from Windows,
Linux, and Android alike (mirrors `mediaway-encoder-vulkan`'s vendor/
framework-axis placement, not an OS backend). See
[ADR-0001](adr/0001-vulkan-video-decode.md).

This ADR's scope is explicitly broader than the sibling encoder's own first
stage: all three codecs (H.264, HEVC, AV1) and **general P/B-frame GOP
support** (DPB/reference-picture-list management), not an IDR-only/all-intra
cut, from the design stage onward — a project-owner decision, see ADR-0001's
Context section.

**Status:** H.264 general-GOP decode is real and **hardware-verified** (hard
pixel-value assertions — the first general-GOP decode backend in this
workspace). HEVC (IDR-only) is also real and **hardware-verified** (hard
pixel-value assertions). AV1 decode is a follow-up.
