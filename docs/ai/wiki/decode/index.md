# Decode

| Doc | Summary |
|-----|---------|
| [scaffold](scaffold.md) | Facade `mediaway-decoder`; `mediaway-decoder::windows` DX11 ZC out |
| [web-video-decode](web-video-decode.md) | Web: `mediaway-decoder::web` WebCodecs `VideoDecoder`; `isConfigSupported` dictionary-vs-boolean bug found + fixed |
| [web-decode-session](web-decode-session.md) | `@mediaway/browser` `DecodeSession` (ADR-0022) — package-level WebCodecs decode wrapper over the wasm `Demuxer`; `resolveCodec` required (container stores generic codec name only) |
| [../platform/vulkan-decode](../platform/vulkan-decode.md) | `mediaway-decoder::vulkan`: H.264 general-GOP decode **hardware-verified** (first in workspace); HEVC IDR decode **hardware-verified** too (P/B still deferred) |
| [windows-decode](../platform/windows-decode.md) | WMF DX11 + CPU decode (H.264 CPU path verified end-to-end 2026-08-05); D3D12 native decode H.264 implemented but **paused** — real GPU-driver TDR hang, root cause narrowed to opaque DXVA blob, not chased further |
| [android-decode](../platform/android-decode.md) | `mediaway-decoder::android` NDK `AMediaCodec` H.264 CPU-output decode — **implemented, zero compile/runtime verification**; general-GOP decode is free (black-box DPB), output color-format is decoder-chosen not caller-requested |
| [../license/sw-opus](../license/sw-opus.md) | `mediaway-sw::opus` — real Opus decode via `unsafe-libopus`, platform-independent SW path; wrapped as `mediaway_decoder::SwOpusAudioDecoder` |
| [../platform/windows-opus](../platform/windows-opus.md) | Windows: inbox WMF Opus decoder MFT (`CMSOpusDecMFT`) — **public** as `mediaway_decoder::windows::WmfOpusDecoder`; verified end-to-end (ffmpeg Opus → exact PCM) |

`AudioDecoder` trait ([`crates/mediaway-decoder/adr/0003-audio-decoder-trait.md`](../../../crates/mediaway-decoder/adr/0003-audio-decoder-trait.md)):
mirrors `VideoDecoder`, implemented by both Opus backends above; no shared
`AudioDecoderConfig` yet (no audio `auto`-dispatch to justify one).

Packaging: [crate-packaging](../meta/crate-packaging.md).
