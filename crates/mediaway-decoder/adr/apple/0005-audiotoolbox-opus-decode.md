# ADR-0005: `AudioToolbox` `AudioConverter` Opus decode

- **Status**: Accepted
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder` (module `mediaway-decoder::apple`)

## ⚠️ Zero real-hardware / zero compile verification in this session

Same constraint as every other Apple ADR this session; see the companion
[encoder ADR-0005](../../../mediaway-encoder/adr/apple/0005-audiotoolbox-opus-encode.md) § header
for the full caveat.

## Context

Two Opus decode paths already exist and are unaffected by this ADR: `mediaway_decoder::windows::
wmf::opus::WmfOpusDecoder` (inbox WMF MFT) and `SwOpusAudioDecoder` (`mediaway-sw`, cross-platform,
`crates/mediaway/src/platform.rs`'s current Apple `decoder_support` fallback). Neither is `Apple`-
native. This ADR adds a third, `AudioConverter`-backed, matching the encoder ADR-0005's rationale
(native over software, mirroring the AAC precedent).

### Symmetric with `AacDecoder`, minus the magic cookie

Reuses `AacDecoder`'s pull-based `AudioConverterFillComplexBuffer` structure unchanged (see
`mediaway-decoder` ADR-0004 and the companion encoder ADR-0005 for the full callback-contract
citation). The one real difference: **no decompression magic cookie**. Opus is self-describing
per-packet (RFC 6716 §3.1's TOC byte encodes configuration/bandwidth/frame-count inline) — this
workspace's own `WmfOpusDecoder` already established the precedent of decoding Opus with
`extra_data: Bytes::new()`, no config record at `open()` at all (confirmed,
`crates/mediaway-decoder/src/windows/wmf/opus.rs`). No local `objc2-audio-toolbox` evidence
suggests `AudioConverter` needs one either — no Opus-specific property exists in the generated
bindings (see encoder ADR-0005's grounding note).

## Decision

> `OpusDecoder::open(config: &OpusDecoderConfig)` — `OpusDecoderConfig { sample_rate, channels,
> time_base }`, deliberately **no `extra_data` field** (unlike `AacDecoderConfig`), matching
> `WmfOpusDecoder`'s existing config shape field-for-field. Source ASBD = `kAudioFormatOpus`
> (sample rate/channels from `config`, `mFramesPerPacket: 0` — converter-resolved, same reasoning
> as the encoder ADR), destination ASBD = interleaved Float32 PCM (identical to `AacDecoder`'s).
> **No `AudioConverterSetProperty` call at all** — `AudioConverterNew` is expected to succeed and
> be immediately ready to decode, since nothing needs to be set before the first
> `AudioConverterFillComplexBuffer` call (contrast `AacDecoder`, which sets the magic cookie in
> this exact spot). `push_packet` enqueues one raw Opus packet into a `VecDeque<PendingPacket>`;
> the input callback pops one packet per invocation with its own
> `AudioStreamPacketDescription { mDataByteSize: packet.len(), .. }`, reusing `AacDecoder`'s
> `consumed: bool` per-call guard unchanged (queue pop happens after `FillComplexBuffer` returns,
> not inside the callback).

### Output frame size is the queried `frame_samples`, not a fixed constant

`AacDecoder` sizes its output scratch buffer from the AAC-fixed `AAC_FRAME_SAMPLES = 1024`. Opus
has no such fixed constant (see encoder ADR-0005's frame-duration discussion) — this decoder
queries `kAudioConverterCurrentOutputStreamDescription` after `AudioConverterNew` the same way the
companion encoder does, and sizes `output_scratch`/`io_output_packet_size` from the resulting
`frame_samples: u32` field instead of a compile-time constant.

### Module layout

```text
src/apple/audiotoolbox/opus.rs   — NEW. OpusDecoder + OpusDecoderConfig, AudioConverter session.
src/apple/audiotoolbox/mod.rs    — CHANGED. mod opus; pub use opus::{OpusDecoder, OpusDecoderConfig};
src/apple/mod.rs                 — CHANGED. Re-exports OpusDecoder/OpusDecoderConfig alongside the
                                    existing AacDecoder/AacDecoderConfig — no wrapper enum, same
                                    direct-exposure precedent `WmfOpusDecoder` already set.
```

`crates/mediaway/src/platform.rs`'s Apple `decoder_support` Opus branch (currently probing
`SwOpusAudioDecoder` live) is updated to probe this native backend instead.

## Scope (this stage)

**In:**

- Raw Opus packet decode to interleaved Float32 PCM, no config record required at `open()`.
- `OpusDecoderConfig { sample_rate, channels, time_base }` — matches `WmfOpusDecoder`'s shape.

**Out (deferred):**

- Frame-duration selection (same gap as the encoder ADR — this backend decodes whatever frame size
  each packet's own TOC byte specifies; there is nothing to "select" on the decode side regardless,
  since Opus decoders always read frame duration from the packet itself, not a caller preference —
  so this is actually a non-issue for decode specifically, noted for symmetry with the encoder ADR
  only).
- `mediaway::platform` wiring beyond the `decoder_support` probe update — no `AppleAudioDecoder`
  wrapper type exists (mirrors `AacDecoder`'s own current scope).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Require a magic cookie / config record at `open()`, mirroring `AacDecoder` | Rejected — no local evidence any Opus `AudioConverter` property for one exists, and `WmfOpusDecoder`'s real, working precedent in this same workspace decodes Opus with zero config record. |
| Hardcode `frame_samples` to 960 (20 ms @ 48 kHz, Opus's most common default) instead of querying `kAudioConverterCurrentOutputStreamDescription` | Rejected — same reasoning as the encoder ADR: an assumed constant is weaker than reading the converter's own resolved ASBD, and costs nothing extra to query once at `open()`. |

## Consequences

### Positive

- Completes a real round trip with the companion encoder ADR-0005 (raw Opus in, raw Opus out, both
  Float32 PCM at the boundary).
- Reuses `AacDecoder`'s entire callback/`AudioBufferList` machinery unchanged.
- Simpler `open()` than `AacDecoder`'s — no magic-cookie step, no `extra_data` requirement.

### Negative / Trade-offs

- **Zero compile verification as authored** — carries over unchanged.
- A third Opus decode path in this workspace (alongside `WmfOpusDecoder` and `SwOpusAudioDecoder`)
  — more surface area to keep consistent, though each targets a distinct platform/backend, not
  genuine duplication.

## References

- `mediaway-encoder` [ADR-apple/0005](../../../mediaway-encoder/adr/apple/0005-audiotoolbox-opus-encode.md) —
  companion encode-direction ADR from the same session.
- `mediaway-decoder` [ADR-apple/0004](0004-audiotoolbox-aac-decode.md) — the `AacDecoder` structural
  template this ADR reuses.
- `mediaway-decoder::windows::wmf::opus::WmfOpusDecoder` — the "no config record needed" precedent
  this ADR follows.
- Local grounding source (read directly): `local/vendor-ref/objc2/generated/CoreAudioTypes/
  CoreAudioBaseTypes.rs` (`kAudioFormatOpus`), `local/vendor-ref/objc2/generated/AudioToolbox/
  AudioConverter.rs` (`kAudioConverterCurrentOutputStreamDescription`).
- `README.md` § Codec support — Apple Opus decode cell: Software → native mark once implemented.

ADRs are written in **English**.
