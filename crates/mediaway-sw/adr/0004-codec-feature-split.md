# ADR-0004: Codec families behind cargo features; low-level Opus / APM block APIs

- **Status**: Accepted
- **Date**: 2026-11-12
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-sw`

## Context

`mediaway-sw` compiled every codec unconditionally: `rav1e` (AV1) and `unsafe-libopus` were
non-optional dependencies, so a consumer that only wants Opus + the audio processor (a game's
voice chat) still built `rav1e`, `av1-grain`, `av-scenechange`, `v_frame`, `rayon`, ….
`apm` / `vad` were already features; AV1, H.264 and Opus were not.

The Opus and APM adapters are also shaped for file-style use: `OpusEncoder::push_frame` takes an
`AudioFrame` and enqueues a `Packet` (two allocations per 20 ms frame), the decoder cannot run
packet-loss concealment or in-band-FEC recovery, and the encoder cannot set complexity, DTX or
the signal type. `AudioProcessor` allocates `Vec`s on every 10 ms block. A real-time voice path
needs none of those allocations and does need those controls.

## Decision

1. **Features** (additive, `default = ["av1", "h264", "opus"]` so existing consumers do not change):
   `av1` (`dep:rav1e`), `h264`, `opus` (`dep:unsafe-libopus`), `apm`, `vad`, and `full` = all five.
   `pcm` is always compiled. A voice consumer uses
   `default-features = false, features = ["opus", "apm", "vad"]` and no longer builds `rav1e`.
2. **Low-level, allocation-free entry points** next to the existing convenience layer (the
   convenience layer is built on the same libopus / sonora calls and is unchanged):
   - `OpusEncoder::encode_float_into(&[f32], &mut [u8]) -> usize`, setters `set_bitrate`,
     `set_complexity`, `set_dtx`, `set_signal`, `set_max_bandwidth`, `set_vbr`, `set_inband_fec`,
     `set_packet_loss_percent`, getters `lookahead`, `in_dtx`; enums `OpusSignal`, `OpusBandwidth`.
     (`OpusEncoderConfig` is a struct other crates build by literal, so new knobs are runtime
     setters, not new fields.)
   - `OpusDecoder::decode_float_into(Option<&[u8]>, &mut [f32], fec) -> usize` (normal decode,
     PLC with `None`, FEC recovery with `fec = true`), `packet_samples`, `reset`.
   - `AudioProcessor::push_render_block` / `process_capture_block` (interleaved `f32`, exactly one
     10 ms block, 1 or 2 channels, `catch_unwind` + passthrough on panic like the frame API),
     `statistics()` (`ApmStats` = `sonora::AudioProcessingStats`: ERLE, delay estimate, ...),
     `capture_block_frames`, `render_block_frames`.
   - `VoiceActivityDetector::analyze_block(&[f32])` (mono `f32` in `[-1, 1]`; the 16-bit scale the
     RNN VAD needs is applied internally).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Split into `mediaway-sw-opus`, `mediaway-sw-av1` crates | ADR-0021 merged the per-codec crates on purpose; features keep one crate and one version. |
| Add fields to `OpusEncoderConfig` | It is constructed by struct literal in `mediaway-encoder` / `mediaway-decoder`; a field would be a breaking change for no gain over setters. |
| Make `default = []` | Would break every current consumer (`mediaway-encoder` needs AV1). Defaults keep today's behavior. |

## Consequences

### Positive

- `cargo tree -p mediaway-sw --no-default-features --features opus,apm,vad` has no `rav1e` subtree.
- Real-time callers can run encode / decode / APM with zero per-frame allocation.
- Loss concealment (PLC, in-band FEC recovery) is reachable without a second Opus binding.

### Negative / Trade-offs

- Another public surface to keep stable; the low-level methods are `0.x` like the rest.
- `unsafe-libopus` is still libopus 1.3.1 (no deep PLC / DRED / LACE); a newer codec can be
  added behind the same shape later.

## References

- [`adr/opus/0001`](opus/0001-unsafe-libopus-encode-decode.md), [`adr/apm/0001`](apm/0001-sonora-audio-processing-adoption.md)
- `docs/ai/wiki/audio/apm.md`
