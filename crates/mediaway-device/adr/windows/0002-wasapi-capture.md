# ADR-0002: WASAPI mic / loopback capture

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device-windows`

## Context

Device roadmap needs microphone, system loopback, and per-process loopback. Personal recorders (`live-recorder`, `sound_capture`) and Eddy Native use WASAPI: default endpoints, IEEE float, worker thread + bounded PCM queue. Process loopback needs `ActivateAudioInterfaceAsync` + `VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK`.

## Decision

> Expose [`WindowsWasapiCapture::open`](../src/wasapi.rs) implementing facade [`AudioCapture`](../../mediaway-device/src/audio.rs):
>
> - Sources: microphone (`eCapture`), render **loopback** (`AUDCLNT_STREAMFLAGS_LOOPBACK`), or **process loopback** (`ActivateAudioInterfaceAsync`).
> - Mic / system loopback: accept **IEEE float** mix format only.
> - Process loopback: fixed **48 kHz stereo float** (GetMixFormat unsupported); see [`wasapi_process.rs`](../src/wasapi_process.rs).
> - Worker thread owns `IAudioClient`; main thread `poll_frame` pops a bounded queue (drop-oldest when full).
> - Stage 1 copies PCM into queued `Bytes` (**not** README ⚡). A later shared/borrowed buffer path can earn **CPU↔CPU ⚡**.
> - No `cpal` / third-party WASAPI crate — `windows` crate COM only (license graph).

Patterns adapted from owned prototypes; re-implemented under Mediaway MIT OR Apache-2.0.

## Consequences

- README mic stays 🆗 until shared-buffer Zero-Copy lands ([wiki marks](../../../docs/ai/wiki/zero-copy/marks.md)).
- Process loopback requires Windows 10 2004+; silent streams are valid when the target is quiet.

## Stage 2 evaluation (shared-buffer CPU ⚡) — not adopted

Evaluated borrowing the `IAudioCaptureClient::GetBuffer` pointer directly (e.g. a custom
`Bytes` vtable deferring `ReleaseBuffer` to `Drop`) instead of copying into `Bytes` in
[`pump_capture_loop`](../src/wasapi.rs). **Not adopted** — two constraints make it a net
regression, not a Zero-Copy win, under the current facade contract:

- `AudioCapture::poll_frame` ([`mediaway-device/src/audio.rs`](../../mediaway-device/src/audio.rs))
  returns an owned `AudioFrame` with no `release_frame`-equivalent lifetime hook, so a
  borrowed buffer would need to defer `ReleaseBuffer` until the caller drops the frame —
  arbitrarily long, from WASAPI's point of view.
- WASAPI shared-mode capture disallows a second `GetBuffer` until the previous one is
  released. Deferring release would collapse the bounded, drop-oldest `PCM_QUEUE_CAP`-frame
  queue to a single in-flight packet: a slow consumer trades graceful oldest-frame drop for
  audio-engine overrun risk.

Genuine CPU ⚡ here would need a facade-level change (an explicit audio frame-release
mechanism, mirroring video's `release_frame`) — out of scope for this platform crate alone
and not pursued in this pass. The mark stays **🆗, not ⚡**.

What did change: [`copy_pcm_buffer`](../src/wasapi.rs) replaced a `vec![0u8; len]` zero-fill
+ `copy_nonoverlapping` pair (two full write passes over the buffer per period) with a
single write into an uninitialized allocation. Still one logical copy — required by the
`GetBuffer`/`ReleaseBuffer` lifetime rule above — but half the write traffic for it.

### Re-confirmed (2026-08-20): current state accepted, not revisited

Re-examined per a request to minimize mic/speaker copies further. Both directions are at
the practical floor already: one copy per period, symmetric with
[`WindowsWasapiPlayback`](0005-wasapi-playback.md)'s own confirmed floor, and the
`AudioCapture`/`AudioPlayback` facade traits (`mediaway-device/src/audio/{capture,playback}.rs`)
still have no `release_frame`-equivalent lifetime hook — this Stage-2 evaluation's premise is
unchanged. Going lower would mean the same cross-cutting facade change (affecting every
platform audio backend: Windows/Apple/Linux/Android), traded against collapsing the capture
queue to depth 1 and real audio-engine overrun risk for a slow consumer. Decision: **accept
the current one-copy-per-period floor**, do not pursue the trait change. Revisit only if a real
caller reports the current queue-based copy as an actual measured bottleneck.

## References

- Facade ADR-0001 · live-recorder / sound_capture (reference only)
