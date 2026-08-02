# ADR-0005: WASAPI shared-mode render playback

- **Status**: Accepted
- **Date**: 2026-07-31
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device-windows`

## Context

Facade ADR-0004 (`mediaway-device`) adds `AudioPlayback` — a push trait
backed by a bounded queue and a platform worker thread, mirroring
`AudioCapture`'s shape in the opposite data direction. This ADR is the
Windows backend for it, mirroring [`WindowsWasapiCapture`](../src/wasapi.rs)
(ADR-0002) as closely as WASAPI's render API allows.

WASAPI render is the mirror image of WASAPI capture at the API level:
`IAudioClient::Initialize` in render mode (no `AUDCLNT_STREAMFLAGS_LOOPBACK`,
`eRender`/`eConsole` default endpoint — the same enumerator call
`wasapi.rs` already uses for `AudioCaptureSource::Loopback`) plus
`IAudioRenderClient::GetBuffer`/`ReleaseBuffer` instead of
`IAudioCaptureClient::GetBuffer`/`ReleaseBuffer`. The buffer-lifetime rule is
symmetric: the pointer `GetBuffer` returns is only valid until the matching
`ReleaseBuffer`, and the caller — not the OS — must fully populate it (or
mark it silent) before releasing.

## Decision

> Add `WindowsWasapiPlayback::open` in a new `src/wasapi_playback.rs`,
> implementing facade `AudioPlayback`, structured like
> `WindowsWasapiCapture`: worker thread owns `IAudioClient` +
> `IAudioRenderClient`; main thread `write_frame` pushes into a bounded
> `VecDeque<AudioFrame>` behind the same `Mutex`+`Arc` shape `wasapi.rs`
> already uses; `close()` blocks on `JoinHandle::join`.

### Endpoint / format negotiation (mirrors capture exactly)

- `GetDefaultAudioEndpoint(eRender, eConsole)` for `device_index == 0`;
  any other index → `PlaybackError::Unsupported` (v1 scope, same restriction
  `wasapi.rs` already applies to `Microphone`/`Loopback`).
- `IAudioClient::GetMixFormat` on the render endpoint; reject anything that
  isn't IEEE float (`WAVE_FORMAT_IEEE_FLOAT` or `WAVE_FORMAT_EXTENSIBLE` +
  `KSDATAFORMAT_SUBTYPE_IEEE_FLOAT`) with `PlaybackError::Unsupported` —
  same `read_float_mix`-shaped check `wasapi.rs` already has, reused rather
  than re-derived.
- `Initialize(AUDCLNT_SHAREMODE_SHARED, 0 /* no loopback flag */, ...,
  format_ptr, None)`. Shared mode only in v1 (facade ADR-0004 Deferred).
- The negotiated `sample_rate`/`channels` from `GetMixFormat` become
  `stream_info()`; `write_frame` payloads must match exactly — no resample
  (facade ADR-0004 Deferred).

### Worker loop: timer-poll, not event-driven (v1 choice)

MSDN's own WASAPI render samples typically favor the **event-driven** mode
(`AUDCLNT_STREAMFLAGS_EVENTCALLBACK` + `SetEventHandle` +
`WaitForSingleObject` on a dedicated thread) because it minimizes the risk of
missing a render deadline. This ADR deliberately does **not** adopt that for
v1: `wasapi.rs`'s capture loop already uses a **timer-poll** shape
(`GetNextPacketSize` + `sleep(5ms)` when idle) that is real and
hardware-verified in this crate today. Mirroring that shape for playback —
poll `IAudioClient::GetCurrentPadding()` on a short timer, compute
`available_frames = buffer_frame_count - padding`, call
`IAudioRenderClient::GetBuffer(available_frames)` — reuses a proven pattern
instead of introducing a second, unverified worker-thread shape
(`WaitForSingleObject` + `SetEventHandle`) in the same crate for v1.

**Honest trade-off**: timer-poll is more underrun-prone than event-driven
render under load, because it does not wake precisely when the engine's
buffer empties. This is a real cost, not hidden — see Deferred. If real
hardware testing shows underrun rates that make timer-poll unusable for
playback specifically (unlike capture, where a late poll only delays
delivery, a late poll here risks the engine actually running dry), the
event-driven variant is the documented next step, not a redesign.

### Underrun / silence-fill mechanics

Two distinct cases per render period, grounded in the real WASAPI buffer
contract (`ReleaseBuffer`'s `AUDCLNT_BUFFERFLAGS_SILENT` flag applies to an
**entire** packet, not a partial span):

| Case | Behavior |
|------|----------|
| Full underrun — queue has nothing for this period | `ReleaseBuffer(available_frames, AUDCLNT_BUFFERFLAGS_SILENT)` — no memset needed; WASAPI's native silence signal for the whole packet. |
| Partial underrun — queue has less than `available_frames` needs | Copy the queued bytes into the front of the buffer, explicitly zero-write the remaining tail (`SILENT` cannot express "half the packet"), then `ReleaseBuffer(available_frames, 0)`. |
| No underrun | Copy queued bytes to fully cover `available_frames`, `ReleaseBuffer(available_frames, 0)`. |

`underrun_count` increments **once per period** that hit either underrun
row above (period granularity, not per-frame) — simplest counter to reason
about for v1 telemetry; finer-grained counting is not needed yet.

### Zero-Copy status (CPU ⚡)

Same status and same root cause as `WindowsWasapiCapture` (ADR-0002
addendum): **not** CPU Zero-Copy, for the mirrored reason.
`IAudioRenderClient::GetBuffer`'s pointer is only valid until the matching
`ReleaseBuffer`, so the queued PCM must be copied into the OS-owned render
buffer — the render engine buffer is always the write target, Mediaway
cannot hand it a pointer to caller-owned memory instead. One copy per
period is the honest floor, symmetric with capture's one copy per period
out of `IAudioCaptureClient::GetBuffer`.

What differs favorably from capture: `AudioPlayback::write_frame` pushing
into the internal queue costs **zero additional copies** — `AudioFrame::data`
is `bytes::Bytes` (refcounted), so queuing is a move + refcount bump, not a
`memcpy`. The unavoidable copy is pushed as late as possible (worker →
render buffer), not duplicated at the submission boundary.

### COM lifecycle / thread shape

Reuses the existing `ComGuard` (`pub(crate)` in `wasapi.rs`,
`CoInitializeEx(COINIT_MULTITHREADED)` + `CoUninitialize` on drop) on the new
playback worker thread — same RAII shape, no new COM abstraction needed.
`close()` sets an `AtomicBool` stop flag the worker observes each poll
iteration, then `JoinHandle::join()`s — blocking the caller until the worker
has called `IAudioClient::Stop()` and released its COM interfaces, mirroring
`WindowsWasapiCapture::close`. `Drop for WindowsWasapiPlayback` calls
`close()`, same as capture.

### Non-Windows stub

`host_stub.rs` (used for `#[cfg(not(windows))]` builds of this crate) gains
a `WindowsWasapiPlayback` stub alongside its existing `WindowsWasapiCapture`
stub, keeping the crate's public surface identical across host platforms.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Event-driven render (`SetEventHandle`) from the start | Real WASAPI-recommended pattern for render specifically, but a second, unverified worker-thread shape in this crate; timer-poll reuses `wasapi.rs`'s already-hardware-verified pattern for v1. Documented as the likely next step if underrun rates demand it, not silently dismissed. |
| Borrow the WASAPI render buffer directly instead of copying from the queue | Same rejection as ADR-0002's capture Stage-2 evaluation: the buffer is only valid between `GetBuffer`/`ReleaseBuffer`, and the render engine always targets its own buffer — there is no API to hand it caller-owned memory instead. |
| `cpal` | Rejected at the facade level (ADR-0004) for the same native-backend, deps-policy, and license-graph-parity reasons as capture. |

## Consequences

### Positive

- Reuses proven pieces: `ComGuard`, the `Arc<Mutex<VecDeque<_>>>` queue
  shape, the timer-poll worker pattern, and the `GetMixFormat`/IEEE-float
  rejection check — all already hardware-verified for capture in this crate.
- Honest, WASAPI-grounded underrun behavior (native `SILENT` flag for full
  periods, explicit zero-fill only where WASAPI cannot express partial
  silence) rather than a blanket memset-everything shortcut.

### Negative / Trade-offs

- Timer-poll render is more underrun-prone under scheduler pressure than
  event-driven render — an honest, documented v1 limitation, not resolved
  here.
- `close()` not draining the queue means any audio queued right before
  `close()` is simply lost, not played out.

## References

- Facade [ADR-0004](../../mediaway-device/adr/0004-audio-playback-traits.md) — trait shape, error type, buffering contract this implements
- [ADR-0002](0002-wasapi-capture.md) — mirrored capture implementation (queue shape, IEEE-float check, `ComGuard`, close-blocks-on-join)
- `wasapi.rs`, `capabilities.rs` (`eRender`/`eConsole`/`EnumAudioEndpoints` already in use)
- [`docs/ai/wiki/device/windows-audio.md`](../../../docs/ai/wiki/device/windows-audio.md) — to be extended once implemented

Crate-local only. Workspace ADRs: [`docs/adr/`](../../../docs/adr/).
