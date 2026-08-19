# ADR-0005: `AudioToolbox` `AudioConverter` Opus encode

- **Status**: Accepted
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (module `mediaway-encoder::apple`)

## ⚠️ Zero real-hardware / zero compile verification in this session

Same structural constraint as every other Apple ADR this session — see ADR-0004 § header for the
full statement. New source read for this ADR:
`local/vendor-ref/objc2/generated/CoreAudioTypes/CoreAudioBaseTypes.rs` (`kAudioFormatOpus`) and
`local/vendor-ref/objc2/generated/AudioToolbox/AudioConverter.rs`
(`kAudioConverterCurrentOutputStreamDescription`). No Opus-specific `AudioToolbox`/`AudioConverter`
property exists anywhere in the generated bindings (confirmed: grepped every file under
`AudioToolbox/` for "opus", zero hits) — this backend relies entirely on generic `AudioConverter`
machinery, the same one ADR-0004 already grounded for AAC.

## Context

`CodecKind::Opus` already has one Apple-reachable backend: `SwOpusAudioEncoder`
(`mediaway-sw`-backed, cross-platform, wired into `AppleAudioEncoder`'s `AudioBackend::Opus` variant
by ADR-0004). `kAudioFormatOpus = 0x6f707573` (confirmed, `CoreAudioBaseTypes.rs`, "Opus codec, has
no flags") is a real, plain `AudioFormatID` — meaning `AudioConverterNew` can target Opus exactly
the same way ADR-0004 already used it for AAC, no new binding/feature needed
(`kAudioFormatOpus` lives in `objc2-core-audio-types`' already-enabled `CoreAudioBaseTypes` feature).

### Why native, given a working `SwOpusAudioEncoder` already exists

User request (this session): make native Opus real, not just the software stand-in. A native
`AudioConverter` path gets Apple's own (frequently hardware-accelerated on newer chips) Opus
encoder instead of `unsafe-libopus`'s pure-software path — same rationale ADR-0004 gave for AAC
over a hypothetical software AAC encoder.

### The one real unknown this ADR cannot resolve locally: frame duration

`SwOpusAudioEncoder` treats `AudioEncoderConfig::time_base` as the caller's **frame-duration
selector** (2.5/5/10/20/40/60 ms, `mediaway_sw::opus::config::OpusEncoderConfig`'s own contract —
confirmed, `crates/mediaway-encoder/src/audio/sw_opus.rs`). No `AudioConverter` property in the
local `objc2-audio-toolbox` checkout lets a caller request a specific Opus frame duration — unlike
AAC's frame size (1024 samples, spec-fixed, so no query was ever needed), Opus's frame size is
genuinely variable and, for this native backend, **entirely converter-chosen**. This is a real,
disclosed behavior difference from `SwOpusAudioEncoder`, not an oversight — see § Scope.

## Decision

> `AacEncoder`'s exact shape, reused nearly verbatim: `OpusEncoder` accepts `SampleFormat::F32`
> interleaved PCM, source ASBD = Float32 PCM (identical to AAC's), destination ASBD =
> `kAudioFormatOpus` with only `mSampleRate`/`mFormatID`/`mChannelsPerFrame` set and
> **`mFramesPerPacket` left at `0`** (unlike AAC's fixed `1024`) — after `AudioConverterNew`
> succeeds, this backend queries `kAudioConverterCurrentOutputStreamDescription` once to read back
> the converter's actual chosen `mFramesPerPacket`, storing it as a per-session `frame_samples: u32`
> field used everywhere `AacEncoder` used its `AAC_FRAME_SAMPLES` constant (output-packet duration,
> `pts` bookkeeping, the `io_output_packet_size` request each `AudioConverterFillComplexBuffer`
> call). `AudioEncoderConfig::time_base` continues to serve only as the tick-conversion unit for
> `pts`/`duration` math (matching every other backend in this workspace) — it does **not** select
> the frame duration for this backend, a documented gap versus `SwOpusAudioEncoder` (see § Scope).
> No magic-cookie/extradata step — Opus is self-describing per-packet (RFC 6716 §3.1's TOC byte),
> matching this workspace's existing `mediaway-decoder::windows::wmf::opus::WmfOpusDecoder`
> precedent (`extra_data: Bytes::new()`); `AudioConverterSetProperty(kAudioConverterEncodeBitRate)`
> is reused unchanged from `AacEncoder` (a generic property, not AAC-specific — confirmed by its own
> doc comment, which does not scope it to any one codec).

### Module layout

```text
src/apple/audiotoolbox/opus.rs   — NEW. OpusEncoder: AudioEncoder impl, AudioConverter session.
src/apple/audiotoolbox/mod.rs    — CHANGED. mod opus; pub(crate) use opus::OpusEncoder;
src/apple/mod.rs                 — CHANGED. AudioBackend::Opus now wraps
                                    audiotoolbox::OpusEncoder (was crate::SwOpusAudioEncoder).
```

`SwOpusAudioEncoder` is not deleted — it remains this workspace's only Windows/Linux/Web Opus
encoder and stays directly constructible by any caller; only `AppleAudioEncoder`'s dispatch target
changes. `crates/mediaway/src/platform.rs`'s Apple `encoder_support` Opus branch (currently probing
`SwOpusAudioEncoder` live) is updated to probe the native backend instead, mirroring how it already
tracks whichever backend `AppleAudioEncoder` actually dispatches to.

## Scope (this stage)

**In:**

- `SampleFormat::F32` PCM input only (matches ADR-0004's AAC scope, and `SwOpusAudioEncoder`'s own
  `F32`-only contract).
- Converter-chosen frame duration, discovered via `kAudioConverterCurrentOutputStreamDescription`
  rather than requested.
- `AppleAudioEncoder`'s `AudioBackend::Opus` now dispatches to this native backend on Apple targets.

**Out (deferred):**

- **Caller-selectable frame duration** — `AudioEncoderConfig::time_base`'s frame-duration-selector
  role (as `SwOpusAudioEncoder` honors it) is not implemented here; no local evidence of a
  `AudioConverter` property for it exists. A caller that specifically needs a chosen Opus frame
  size (e.g. matching a WebRTC 20 ms convention exactly) should use `SwOpusAudioEncoder` directly
  instead of going through `AppleAudioEncoder`.
- `SampleFormat::S16`/`S32` input — same gap ADR-0004 already carries for AAC.
- Opus-specific tuning (complexity, `OPUS_APPLICATION_*`, in-band FEC, DTX) — no corresponding
  `AudioConverter` properties found locally; `SwOpusAudioEncoder` remains the path for callers who
  need `OpusEncoderConfig`'s explicit `application`/`inband_fec`/`packet_loss_percent` knobs.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Keep `AppleAudioEncoder::Opus` on `SwOpusAudioEncoder` permanently | Rejected per explicit user request this session — native (frequently HW-accelerated) Opus is the stated goal, matching AAC's own native-over-software precedent (ADR-0004). |
| Hardcode a frame-duration constant (e.g. 960 samples = 20 ms @ 48 kHz) instead of querying `kAudioConverterCurrentOutputStreamDescription` | Rejected — unlike AAC's spec-fixed 1024, nothing in the local `objc2` checkout confirms Opus's `AudioConverter` default duration; querying the converter's own resolved ASBD is the only locally-grounded way to learn it, and is exactly the pattern `AudioConverterNew`'s own doc comment recommends ("the actual formats … can be obtained by getting the properties … from it"). |
| Delete `SwOpusAudioEncoder`'s Apple usage entirely, forcing all Apple callers onto the native backend | Rejected — the frame-duration and Opus-tuning gaps above are real; a caller needing those still has `SwOpusAudioEncoder` directly (not removed, just no longer `AppleAudioEncoder`'s default). |

## Consequences

### Positive

- No new Cargo dependency/feature — `kAudioFormatOpus` is already reachable through
  `objc2-core-audio-types`'s `CoreAudioBaseTypes` feature, already enabled for AAC.
- Reuses `AacEncoder`'s entire callback/`AudioBufferList`-construction machinery essentially
  unchanged — the only structural addition is the post-open ASBD query for `frame_samples`.
- Matches the native-over-software precedent ADR-0004 already established for AAC.

### Negative / Trade-offs

- **Zero compile verification as authored** — carries over unchanged.
- Frame duration is no longer caller-selectable through `AppleAudioEncoder` — a real, disclosed
  behavior difference from `SwOpusAudioEncoder`'s existing contract on other platforms.
- Opus-specific tuning knobs (`application`, in-band FEC, DTX, packet-loss-percent hinting) are
  unavailable through this backend — `SwOpusAudioEncoder` remains the only path for them.

## References

- `mediaway-encoder::apple::audiotoolbox::aac::AacEncoder` — the structural template this ADR
  reuses nearly verbatim.
- `mediaway-encoder::audio::sw_opus::SwOpusAudioEncoder` — the existing cross-platform backend this
  ADR does not remove, only stops being `AppleAudioEncoder`'s default.
- `mediaway-decoder` [ADR-apple/0005](../../../mediaway-decoder/adr/apple/0005-audiotoolbox-opus-decode.md) —
  companion decode-direction ADR from the same session.
- Local grounding source (read directly): `local/vendor-ref/objc2/generated/CoreAudioTypes/
  CoreAudioBaseTypes.rs` (`kAudioFormatOpus`), `local/vendor-ref/objc2/generated/AudioToolbox/
  AudioConverter.rs` (`kAudioConverterCurrentOutputStreamDescription`, `AudioConverterNew`'s doc
  comment on querying resolved formats).
- `README.md` § Codec support — Apple Opus cell: Software → native mark once implemented.

ADRs are written in **English**.
