# Windows AAC decode (WMF)

- **Session**: inbox WMF AAC decoder MFT (`CLSID_CMSAACDecMFT`,
  `{32d186a7-218f-4c75-8876-dd77273a8999}`), Float32 PCM out.
- **Entry**: `mediaway_decoder::windows::WmfAacDecoder` (+ `AacDecoderConfig`) —
  implements `AudioDecoder` ([ADR-0003](../../../crates/mediaway-decoder/adr/0003-audio-decoder-trait.md)),
  usable standalone. Policy: [ADR-0006](../../../crates/mediaway-decoder/adr/windows/0006-wmf-aac-decode.md).
  `decoder_support(Aac)` probes it live.
- **Verified** (2026-09-18): 4096 PCM samples/ch → this workspace's WMF AAC encoder →
  4 AAC-LC packets (ASC `[11 90]`) → `WmfAacDecoder` → 4 frames, **4096 samples/ch,
  peak 1.0022**. Sample-exact round trip of a 440 Hz sine; README Windows AAC decode
  `👻 → ✅`.

## The `AudioSpecificConfig` is mandatory

Unlike the Opus MFT (rate + channels is enough), the AAC MFT needs `MF_MT_USER_DATA` =
12-byte `HEAACWAVEINFO` tail + raw ASC
([MS reference](https://learn.microsoft.com/en-us/windows/win32/medfound/aac-decoder)):

| offset | size | field | used |
|---|---|---|---|
| 0 | 2 | `wPayloadType` | 0 (raw AAC) |
| 2 | 2 | `wAudioProfileLevelIndication` | 0xFE (unspecified) |
| 4 | 2 | `wStructType` | 0 |
| 6 | 2 | `wReserved1` | 0 |
| 8 | 4 | `dwReserved2` | 0 |
| 12 | n | `AudioSpecificConfig` | caller-supplied |

Empty `extra_data` → `DecodeError::Unsupported`. **Do not synthesize an ASC** from
rate/channels: for SBR/PS streams the media type describes the *core* stream before those
tools apply, so the pair does not determine the config and a guess decodes to quietly
wrong output. Same required-ASC posture as `apple::AacDecoder` and this crate's VP9/AV1
video decoders.

For AAC in MP4 the ASC is the `esds` DecoderSpecificInfo, which `iso-bmff` already
captures into `StreamInfo::extra_data`.

## Gotchas

- **Output type is not index 0.** The AAC MFT registers *both* Float32 and 16-bit PCM, so
  `GetOutputAvailableType(0, 0)` may hand back 16-bit. `aac.rs` enumerates and selects
  `MFAudioFormat_Float`; `opus.rs` can take index 0 only because that MFT offers one type.
- **Raw AAC only** — `wPayloadType` is hardcoded 0. ADTS (1) and LOAS/LATM (3) are
  deliberately not sniffed per packet; use `adts-core` to strip headers first.
- **AAC-LC 1024-sample frames only**; the MFT rejects the 960-sample variant.
- `wmf::audio_mft` holds the sync-MFT plumbing shared with [windows-opus](windows-opus.md)
  (output-sample allocation, `NEED_MORE_INPUT` handling, buffer copy). Separate from
  `wmf::shared`, which is video-feature-gated.

## Still open

- Windows AAC **encode** stays `🆗`: the round trip decodes a Microsoft encoder with
  Microsoft's own decoder, which is not independent conformance evidence. Upgrading needs
  the `ffmpeg` oracle ([ADR-0002](../../../docs/adr/0002-system-oracle.md)).
- `mediaway-ffi`'s audio decode C ABI is still Opus-only (`pipeline/audio_decoder.rs`
  wraps `mediaway_sw::opus::OpusDecoder` directly); its module doc's claim that no
  `AudioDecoder` trait exists is stale since ADR-0003.
- `platform::decoder_support` has no AAC arm on **Apple**, although `apple::AacDecoder`
  exists — same gap this change closed on Windows.
