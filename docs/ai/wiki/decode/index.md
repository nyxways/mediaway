# Decode

| Doc | Summary |
|-----|---------|
| [scaffold](scaffold.md) | Facade `mediaway-decoder`; `decoder-windows` DX11 ZC out |
| [web-video-decode](web-video-decode.md) | Web: `mediaway-decoder-web` WebCodecs `VideoDecoder`; `isConfigSupported` dictionary-vs-boolean bug found + fixed |
| [../platform/vulkan-decode](../platform/vulkan-decode.md) | `mediaway-decoder-vulkan`: H.264 general-GOP decode **hardware-verified** (first in workspace); HEVC sans-io real+tested, GPU decode still all-zero (unresolved) |
| [windows-decode](../platform/windows-decode.md) | WMF DX11 decode + D3D12 native decode (H.264 implemented; **paused** — real GPU-driver TDR hang, root cause narrowed to opaque DXVA blob, not chased further) |
| [../license/sw-opus](../license/sw-opus.md) | `mediaway-sw-opus` — real Opus decode via `unsafe-libopus`, platform-independent SW path |
| [../platform/windows-opus](../platform/windows-opus.md) | Windows: inbox WMF Opus decoder MFT (`CMSOpusDecMFT`) — **public** as `mediaway_decoder::windows::WmfOpusDecoder`; verified end-to-end (ffmpeg Opus → exact PCM) |

Packaging: [crate-packaging](../meta/crate-packaging.md).
