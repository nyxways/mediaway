# ADR-0004: `AudioToolbox` `AudioConverter` AAC decode

- **Status**: Accepted
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-decoder` (module `mediaway-decoder::apple`)

## ⚠️ Zero real-hardware / zero compile verification in this session

Same constraint as every other Apple ADR this session; see the companion
[encoder ADR-0004](../../../mediaway-encoder/adr/apple/0004-audiotoolbox-aac-encode.md) § header
for the full caveat and the same "locally-grounded vs. public-reference-informed" distinction,
which applies here identically.

## Context

**No AAC decoder exists anywhere in this workspace before this ADR — not even on Windows.**
Confirmed: `mediaway-decoder::windows` has no AAC module (only `WmfOpusDecoder`); grepping the
whole crate for `Aac` found nothing before this change. This ADR's `AacDecoder` is the first AAC
decode backend in this workspace, on Apple, ahead of Windows. `AudioDecoder` (`stream_info`,
`push_packet`, `poll_frame`, `flush`) is already a generic trait (confirmed, `src/audio.rs`) —
unlike encode, there is **no** shared `AudioDecoderConfig` type; every existing audio decode
backend defines its own config struct (`windows::wmf::opus::OpusDecoderConfig`,
`mediaway_sw::opus::config::OpusDecoderConfig` — two distinct types with the same name in
different modules, confirmed while researching the companion encoder-support Opus wiring earlier
this session). This ADR follows that same per-backend-config precedent: a new `AacDecoderConfig`.

### Same pull-based shape as encode, reversed

`AudioConverterFillComplexBuffer` is symmetric — this ADR reuses the encoder ADR-0004's fully-cited
callback contract (starvation signaling, single-thread nesting, buffer-validity window) unchanged,
just with source/destination ASBDs swapped and the callback supplying **compressed AAC packets**
(with real per-packet `AudioStreamPacketDescription`s, since compressed packets vary in byte size)
instead of uniform-stride PCM.

### The decompression magic cookie must be set *before* decoding, not read after

Encode discovers its magic cookie *from* the converter after encoding starts (the codec picks its
own ASC). Decode is the reverse: `kAudioConverterDecompressionMagicCookie` (confirmed,
`AudioConverter.rs`) must be **set** via `AudioConverterSetProperty` — the converter needs the
`AudioSpecificConfig` to know the AAC profile/sample-rate-table/channel-configuration before it
can decode anything. This ADR therefore requires `extra_data` (the raw ASC) to be supplied in
`AacDecoderConfig` **at `open()`**, mirroring this session's VP9/AV1 video-decode ADR's identical
"container must supply the config record up front, no in-band discovery" pattern — the same
class of constraint, now on the audio side.

## Decision

> `AacDecoder::open(config: &AacDecoderConfig)` requires `config.extra_data` (raw ASC) non-empty —
> `DecodeError::Unsupported` if empty. Builds source ASBD = `kAudioFormatMPEG4AAC` (sample
> rate/channels from `config`, rest zeroed — same "trust the codec's own resolution" posture as
> the encoder ADR), destination ASBD = interleaved Float32 PCM (this backend's sole output
> format, matching the encoder's sole input format — a deliberately symmetric round-trip
> capability with the encoder ADR). Sets the magic cookie via `AudioConverterSetProperty
> (kAudioConverterDecompressionMagicCookie, ...)` immediately after `AudioConverterNew` succeeds.
> `push_packet` enqueues one raw (non-ADTS) AAC packet into an internal `VecDeque<Bytes>`; the
> input callback pops one packet per invocation, filling one `AudioStreamPacketDescription
> { mStartOffset: 0, mVariableFramesInPacket: 0, mDataByteSize: packet.len() }` (raw AAC packets
> carry a fixed 1024-frames-per-packet duration at the format level, not per-packet, so
> `mVariableFramesInPacket` is always `0` here — only formats with genuinely variable frame counts
> per packet need it non-zero).

### Raw AAC, not ADTS — an assumption this ADR states explicitly, not a locally-confirmed fact

This workspace's only other AAC surface (the companion encoder ADR-0004) produces **raw** AAC
packets (no ADTS 7-byte header) — matching the MP4/`esds`-first convention `iso-bmff`/
`mediaway-container` already use elsewhere in this workspace (ADTS framing exists for the
`adts-core` crate specifically, a distinct on-the-wire container, not this backend's assumed
input shape). This ADR assumes `push_packet`'s input is symmetric with that — raw AAC packets,
not ADTS-framed ones. A caller feeding ADTS-framed packets would see `AudioConverterFillComplexBuffer`
either reject them or (more likely) silently misdecode, since the 7-byte ADTS header would be
read as AAC payload bytes — **this ADR does not add ADTS stripping**, matching the "no bitstream
parsing beyond what's needed" posture the VP9/AV1 video-decode ADR already established for a
different codec. Flagged as a real, documented scope boundary, not silently assumed.

## Scope (this stage)

**In:**

- Raw (non-ADTS) AAC-LC decode to interleaved Float32 PCM, given a real ASC at `open()`.
- `AacDecoderConfig { sample_rate, channels, time_base, extra_data }` — new, backend-local type
  (matches the existing per-backend audio-decode-config precedent, not a `mediaway-common` change).

**Out (deferred):**

- ADTS-framed input (see above) — a real, documented gap, not silently handled.
- HE-AAC/HE-AACv2 (SBR/PS) — same scope cut as the encoder ADR, for the same reason (trusting
  `AudioConverterNew`'s automatic resolution rather than `AudioConverterNewSpecific`).
- `mediaway::platform` wiring — no `WindowsAudioDecoder`-style facade wrapper exists for any
  platform yet (confirmed while researching this ADR — Windows exposes `WmfOpusDecoder` directly,
  no wrapper enum), so `AacDecoder` is exposed the same direct way, not a new gap.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Derive the magic cookie from the first packet's own bitstream (parse `AudioSpecificConfig` fields out of the raw AAC payload itself) | Rejected — `AudioSpecificConfig` is not embedded in raw AAC frame payloads at all (unlike H.264's in-band SPS/PPS); it is genuinely only available from the container (MP4 `esds`, or whatever transport carried it) — requiring it at `open()` is not a scope-cut, it is the only place this data exists. |
| Reuse `mediaway_sw::opus::config::OpusDecoderConfig`'s field shape/name for `AacDecoderConfig` (both being "the config for a compressed-audio decoder") | Rejected — no shared trait/type ties audio-decode configs together in this workspace (confirmed by the encoder-support Opus research this session), so inventing a cross-codec shared shape here would be new, unrequested architecture; a plain, codec-named struct matches every existing precedent instead. |
| Support ADTS-framed input by stripping the 7-byte header before each `push_packet` | Deferred, not rejected outright — a legitimate future addition once a real ADTS-sourced caller exists; out of scope for this ADR's first cut, which mirrors the encoder's raw-AAC-only output shape for a clean round-trip story. |

## Consequences

### Positive

- Completes a real Zero-Copy-free round trip with the companion encoder ADR (raw AAC in, raw AAC
  out, both Float32 PCM at the boundary) — same codec, same framing convention, verifiable
  against each other once real hardware access exists.
- First AAC decoder in this workspace at all — closes a gap Windows itself still has.
- Reuses the encoder ADR's fully-cited `AudioConverterFillComplexBuffer` callback contract
  unchanged — no new API surface to re-verify.

### Negative / Trade-offs

- **Zero compile verification as authored** — carries over unchanged.
- ADTS-framed input is a real, unhandled gap for now.
- `AacDecoderConfig` is yet another one-off audio-decode config type, matching (not fixing) this
  workspace's existing lack of a shared shape.

## References

- `mediaway-encoder` [ADR-apple/0004](../../../mediaway-encoder/adr/apple/0004-audiotoolbox-aac-encode.md) —
  companion encode-direction ADR from the same session; full `AudioConverter` callback contract
  citation lives there, reused here unchanged
- `mediaway-decoder` [ADR-apple/0002](0002-videotoolbox-hevc-vp9-av1-decode.md) — the VP9/AV1
  "container must supply the config record at `open()`" precedent this ADR mirrors for audio
- Local grounding source (read directly): `local/vendor-ref/objc2/generated/AudioToolbox/
  AudioConverter.rs` (`kAudioConverterDecompressionMagicCookie`, `AudioConverterSetProperty`),
  `local/vendor-ref/objc2/generated/CoreAudioTypes/CoreAudioBaseTypes.rs`
  (`AudioStreamPacketDescription`, `kAudioFormatMPEG4AAC`)
- `README.md` § Codec support — Apple AAC cell: `👻` → `🆗` once implemented

ADRs are written in **English**.
