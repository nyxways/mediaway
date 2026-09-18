# ADR-0006: WMF AAC decode session (`CMSAACDecMFT`)

- **Status**: Accepted
- **Date**: 2026-09-18
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: mediaway-decoder

## Context

This workspace had four `AudioDecoder` implementations and **three of them were Opus**
(`windows::wmf::opus`, `apple::OpusDecoder`, `audio::SwOpusAudioDecoder`); the only AAC
decoder was `apple::AacDecoder`, macOS/iOS only. The README codec table recorded Windows
AAC as `🆗 / 👻` — encode implemented-unverified, decode absent.

That gap is asymmetric in a way that bites: `WindowsAudioEncoder` *does* encode AAC, and
`mediaway-container`'s `mp4a`/`esds` sample entry is the only audio sample entry
`iso-bmff` writes. So a caller on Windows could record AAC into fMP4 through this
workspace and then have no way to play it back with this workspace — audio it could
produce but not consume.

Windows has shipped an inbox AAC decoder MFT since Windows 7
(`CLSID_CMSAACDecMFT`, `{32d186a7-218f-4c75-8876-dd77273a8999}`), and the `windows`
crate already exposes the CLSID and every attribute needed. Nothing blocked this but the
absence of a module.

## Decision

> We adopt a WMF AAC decode session, `windows::wmf::aac::WmfAacDecoder`, mirroring the
> existing `WmfOpusDecoder` shape, with Float32 PCM output and a **required**
> `AudioSpecificConfig`.

### Scope

- **Raw AAC only** (`wPayloadType = 0`): exactly one `raw_data_block()` per `Packet`,
  which is how MP4 stores AAC samples.
- **Float32 output** (`MFAudioFormat_Float`), matching every other audio backend here.
- **ADTS and LOAS/LATM are out of scope.** The MFT supports both (`wPayloadType` 1 and
  3), but this session hardcodes 0. Sniffing per packet would make the input shape
  implicit and silently mis-decode a mislabelled stream; a caller with ADTS has
  `adts-core` to strip headers, and if in-session ADTS is ever wanted it should arrive as
  an explicit config field, not a guess.
- **AAC-LC 1024-sample frames only**, because that is all the MFT accepts (it rejects the
  960-sample variant outright).

### `MF_MT_USER_DATA` layout

Per Microsoft's AAC decoder reference
(<https://learn.microsoft.com/en-us/windows/win32/medfound/aac-decoder>),
`MF_MT_USER_DATA` for `MFAudioFormat_AAC` is the `HEAACWAVEINFO` tail (everything after
the embedded `WAVEFORMATEX`) followed by the `AudioSpecificConfig`:

| offset | size | field | value used |
|---|---|---|---|
| 0 | 2 | `wPayloadType` | 0 (raw AAC) |
| 2 | 2 | `wAudioProfileLevelIndication` | 0xFE (unspecified) |
| 4 | 2 | `wStructType` | 0 |
| 6 | 2 | `wReserved1` | 0 |
| 8 | 4 | `dwReserved2` | 0 |
| 12 | n | `AudioSpecificConfig` | caller-supplied |

`wAudioProfileLevelIndication` is 0xFE rather than a real profile because the profile is
already implied by the `AudioSpecificConfig` that follows; the reference documents 0xFE
as the "no audio profile specified" value.

This layout is independently corroborated in-tree: `mediaway-encoder`'s WMF AAC *encoder*
reads back `[00 00 29 00 00 00 00 00 00 00 00 00 11 90]` for 48 kHz stereo AAC-LC, whose
12-byte prefix matches the table and whose trailing `11 90` is that format's ASC.

### An empty `extra_data` is an error, not a synthesized default

`AacDecoderConfig::extra_data` must be non-empty; empty yields `DecodeError::Unsupported`.

Synthesizing an ASC from `sample_rate` + `channels` is tempting and wrong. For streams
using SBR or PS, the media type's rate and channel count describe the **core** stream
*before* those tools are applied (SBR doubles the output rate; PS decodes stereo from a
mono core), so the pair does not determine the config. A synthesized ASC would open
successfully and decode to quietly wrong output — the worst available failure mode.

This matches `apple::AacDecoder` (`adr/apple/0004`) and this crate's VP9/AV1 video
decoders, which likewise require the container's config record at `open()`.

### Shared MFT plumbing extracted

`opus.rs` and `aac.rs` drive their sync MFTs identically — allocate the output sample
ourselves, `ProcessOutput`, treat `MF_E_TRANSFORM_NEED_MORE_INPUT` as "no frame yet",
copy the locked buffer out. Rather than duplicate ~120 lines, that moved into
`wmf::audio_mft`, leaving each session only its own media-type negotiation. It is
deliberately separate from `wmf::shared`, which is `#[cfg(feature = "video")]`; an
audio-only build must not require the video feature.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Hand-craft a minimal `raw_data_block()` for the test | Would prove only that the MFT tolerates whatever was hand-written. There is no software AAC encoder in this workspace to cross-check against, unlike Opus where `mediaway-sw` provides one. |
| Take `GetOutputAvailableType(0, 0)` like `opus.rs` | The Opus MFT offers exactly one output type; the AAC MFT registers both Float32 and 16-bit PCM, so index 0 is not guaranteed to be the float type. This enumerates and selects `MFAudioFormat_Float`, falling back to the first proposal only if no float type is offered. |
| Accept an empty ASC and synthesize one | See above — silently wrong for SBR/PS. |
| Sniff ADTS vs raw per packet | Makes the input shape implicit; a mislabelled stream mis-decodes instead of failing. |
| A shared `AudioDecoderConfig` across backends | Out of scope and premature — ADR-0003 already recorded that every audio backend defines its own config, and there is still no audio `auto`-dispatch to justify unifying them. |

## Consequences

### Positive

- **Hardware-verified**, not merely implemented. Round trip on a Windows 11 box: 4096 PCM
  samples/channel → this workspace's WMF AAC encoder → 4 AAC-LC packets, ASC `[11 90]` →
  `WmfAacDecoder` → **4 frames, 4096 samples/channel, peak amplitude 1.0022**. A 440 Hz
  sine goes in and comes back as signal, not silence, and the sample count round-trips
  exactly. The README's Windows AAC **decode** cell moves `👻 → ✅`.
- `platform::decoder_support(CodecKind::Aac)` now reports live availability instead of
  falling through to the video decoder path and reporting a misleading video failure.
- `opus.rs` shrank from 405 to 269 lines by sharing the extracted plumbing.

### Negative / Trade-offs

- **The Windows AAC *encode* mark stays `🆗`.** The round trip exercises the encoder too,
  but decoding a Microsoft encoder's output with Microsoft's own decoder is not
  independent conformance evidence — a non-conformant bitstream both sides agree on would
  pass. Upgrading that cell needs a third-party decoder (e.g. the `ffmpeg` oracle,
  ADR-0002).
- Callers must now carry the `AudioSpecificConfig` from the container to the decoder.
  That is correct, but it is a real ergonomic cost versus Opus's rate+channels.
- ADTS input still requires out-of-session header stripping.
- `mediaway-decoder`'s dev-dependency on `mediaway-encoder` gained the `audio` feature, so
  the test build pulls in the audio encode path. Dev-only; no effect on shipped artifacts.

## References

- [AAC Decoder — Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/medfound/aac-decoder)
- [`HEAACWAVEINFO`](https://learn.microsoft.com/en-us/windows/desktop/api/mmreg/ns-mmreg-heaacwaveinfo)
- `adr/0003-audio-decoder-trait.md` — the `AudioDecoder` trait this implements
- `adr/apple/0004-audiotoolbox-aac-decode.md` — the sibling AAC decoder and the
  required-ASC precedent
- `adr/windows/0001-wmf-h264-dx11-out.md` — WMF backend policy
- `docs/ai/wiki/platform/windows-aac.md`
