# Decode

| Doc | Summary |
|-----|---------|
| [scaffold](scaffold.md) | Facade `mediaway-decoder`; `mediaway-decoder::windows` DX11 ZC out |
| [web-video-decode](web-video-decode.md) | Web: `mediaway-decoder::web` WebCodecs `VideoDecoder`; `isConfigSupported` dictionary-vs-boolean bug found + fixed |
| [../platform/vulkan-decode](../platform/vulkan-decode.md) | `mediaway-decoder::vulkan`: H.264 general-GOP decode **hardware-verified** (first in workspace); HEVC IDR decode **hardware-verified** too (P/B still deferred) |
| [windows-decode](../platform/windows-decode.md) | WMF DX11 decode + D3D12 native decode (H.264 implemented; **paused** — real GPU-driver TDR hang, root cause narrowed to opaque DXVA blob, not chased further) |
| [../license/sw-opus](../license/sw-opus.md) | `mediaway-sw::opus` — real Opus decode via `unsafe-libopus`, platform-independent SW path; wrapped as `mediaway_decoder::SwOpusAudioDecoder` |
| [../platform/windows-opus](../platform/windows-opus.md) | Windows: inbox WMF Opus decoder MFT (`CMSOpusDecMFT`) — **public** as `mediaway_decoder::windows::WmfOpusDecoder`; verified end-to-end (ffmpeg Opus → exact PCM) |

`AudioDecoder` trait ([`crates/mediaway-decoder/adr/0003-audio-decoder-trait.md`](../../../crates/mediaway-decoder/adr/0003-audio-decoder-trait.md)):
mirrors `VideoDecoder`, implemented by both Opus backends above; no shared
`AudioDecoderConfig` yet (no audio `auto`-dispatch to justify one).

Packaging: [crate-packaging](../meta/crate-packaging.md).
