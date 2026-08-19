# Decode

| Doc | Summary |
|-----|---------|
| [scaffold](scaffold.md) | Facade `mediaway-decoder`; `mediaway-decoder::windows` DX11 ZC out |
| [web-video-decode](web-video-decode.md) | Web: `mediaway-decoder::web` WebCodecs `VideoDecoder`; `isConfigSupported` dictionary-vs-boolean bug found + fixed |
| [web-decode-session](web-decode-session.md) | `@mediaway/browser` `DecodeSession` (ADR-0022) — package-level WebCodecs decode wrapper over the wasm `Demuxer`; `resolveCodec` required (container stores generic codec name only) |
| [../platform/vulkan-decode](../platform/vulkan-decode.md) | `mediaway-decoder::vulkan`: H.264 general-GOP decode **hardware-verified** (first in workspace); HEVC IDR decode **hardware-verified** too (P/B still deferred) |
| [windows-decode](../platform/windows-decode.md) | WMF DX11 + CPU decode (H.264 CPU path verified end-to-end 2026-08-05); D3D12 native decode H.264 implemented but **paused** — real GPU-driver TDR hang, root cause narrowed to opaque DXVA blob, not chased further |
| [windows-decode-d3d12-hevc](../platform/windows-decode-d3d12-hevc.md) | D3D12 HEVC decode ADR-0004 — **implemented, sans-io-verified only** (42 unit tests, clean check/clippy), single-forward-reference P-slice scope; zero real GPU hardware verification, deliberately — do not run any D3D12 decode hardware test |
| [windows-decode-d3d12-av1](../platform/windows-decode-d3d12-av1.md) | D3D12 AV1 decode ADR-0005 — **implemented, sans-io-verified only** (43 unit tests, clean check/clippy/fmt), `KEY_FRAME`-only Main profile scope; zero real GPU hardware verification, deliberately — do not run any D3D12 decode hardware test; own encoder's output not confirmed decodable |
| [android-decode](../platform/android-decode.md) | `mediaway-decoder::android` NDK `AMediaCodec` H.264 CPU-output decode — **implemented, zero compile/runtime verification**; general-GOP decode is free (black-box DPB), output color-format is decoder-chosen not caller-requested |
| [../platform/apple-decode](../platform/apple-decode.md) | `mediaway-decoder::apple`: `VTDecompressionSession` H.264 general-GOP CPU-output decode — **implemented, zero compile verification**; VideoToolbox owns the DPB + display-order reorder internally |
| [../license/sw-opus](../license/sw-opus.md) | `mediaway-sw::opus` — real Opus decode via `unsafe-libopus`, platform-independent SW path; wrapped as `mediaway_decoder::SwOpusAudioDecoder` |
| [../platform/windows-opus](../platform/windows-opus.md) | Windows: inbox WMF Opus decoder MFT (`CMSOpusDecMFT`) — **public** as `mediaway_decoder::windows::WmfOpusDecoder`; verified end-to-end (ffmpeg Opus → exact PCM) |
| [web-opus-design](web-opus-design.md) | Web: first audio decode surface in `mediaway-decoder::web` (`is_webcodecs_audio_decode_supported`, `decode_audio_chunks`), codec-generalized, exercised via Opus — wasm32 compile-verified only |

`AudioDecoder` trait ([`crates/mediaway-decoder/adr/0003-audio-decoder-trait.md`](../../../crates/mediaway-decoder/adr/0003-audio-decoder-trait.md)):
mirrors `VideoDecoder`, implemented by both Opus backends above; no shared
`AudioDecoderConfig` yet (no audio `auto`-dispatch to justify one).

Packaging: [crate-packaging](../meta/crate-packaging.md).
