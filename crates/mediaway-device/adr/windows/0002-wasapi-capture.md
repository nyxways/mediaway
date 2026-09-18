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

## Correction (2026-09-18): process loopback never worked, and its scope enum was inverted

Two defects in [`wasapi_process.rs`](../../src/windows_audio/wasapi_process.rs), both dating
from the original implementation. Neither was caught because the module mapped every failure
to a bare `CaptureError::Backend`, discarding the `HRESULT`.

**1. `Initialize` was passed flags the mode rejects.** The call used
`AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY` and returned
`AUDCLNT_E_INVALID_STREAM_FLAG` (`0x8889_0021`) on **every** machine, in both MTA and STA
(measured — `ActivateAudioInterfaceAsync` itself succeeds in both, so the apartment was not
the variable). The process-loopback virtual device requires
`AUDCLNT_STREAMFLAGS_LOOPBACK` and accepts no sample-rate-conversion flags: there is no mix
format to convert from, since `GetMixFormat` is unsupported here and the caller supplies
`float_stereo_48k()` outright (§Decision above). Because
[`crate::windows::capabilities`](../../src/windows/capabilities.rs) reuses this function as
its process-loopback support probe, `Support` for `DeviceKind::ProcessLoopback` was
`Unavailable(OsVersionTooOld)` everywhere, including on Windows 11.

Diagnosability was the root cause, so it is fixed alongside: `CaptureError` gained
`BackendCode { code: i32 }` (shape mirrors `mediaway_sw::opus::OpusError::Backend`, the
repo's existing native-code convention) and every WASAPI/COM failure in this module now
carries its `HRESULT`. The regression test can then assert the precise thing — *not*
`AUDCLNT_E_INVALID_STREAM_FLAG* — even on a machine that genuinely lacks the feature, which
a bare `Backend` made impossible.

**2. `ProcessTreeScope::ProcessOnly` selected the inverse mode.** It was documented as "only
audio rendered directly by the target process" and mapped to
`PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE` — "everything but this process tree". It
opened successfully and recorded the wrong audio, silently. Windows offers exactly two modes
and neither is "this process but not its children", so the variant was not implementable as
documented.

> **Decision: rename it to `ExcludeProcessTree` and document it as a "record everything
> else" mode**, rather than removing it and leaving `IncludeChildren` as the only
> process-scoped option.

| Alternative | Why not |
|-------------|---------|
| Remove `ProcessOnly` | Deletes a real Windows mode the backend already implements, for a naming defect. [`docs/spec/api-layers.md`](../../../../docs/spec/api-layers.md) rule 4 keeps platform detail visible at the bottom layer rather than erasing it, and the mode has a concrete use — a recorder capturing system audio *without* its own playback feeding back. It would also leave `ProcessTreeScope` a one-variant enum and force the C ABI's `bool` to be either an ABI break or a silently-ignored field: strictly more churn, less honesty. |
| Keep the name, fix the mapping | Not possible — the mode the name promises does not exist in Windows. |

Downstream impact was weighed and found not to be decisive either way: the one known
consumer (qarec) only ever uses `IncludeChildren` and refused `ProcessOnly` outright, and
defect 1 means *no* caller of either mode has ever had a working session.

Carried through the stack for one meaning per name: `WasapiProcessTreeScope::ProcessOnly` →
`ExcludeProcessTree`, and the C ABI's `include_child_processes` → `include_target_process_tree`
(`false` = `EXCLUDE_TARGET_PROCESS_TREE`, the complement — not a narrowing), with the C#/Python
bindings' parameter names and docs following. Same polarity and same struct layout, so this is
a source-level rename, not an ABI change.

**Pinned by tests**, since both defects were invisible from the outside:
`wasapi_process_tests.rs` asserts each scope → `PROCESS_LOOPBACK_MODE` mapping and opens a
real process-loopback client against the test process;
`windows_desktop/desktop_audio_tests.rs` asserts the facade → backend scope translation; and
`windows_desktop/lib_tests.rs`'s pre-existing "or skip" session test now fails loudly on
`AUDCLNT_E_INVALID_STREAM_FLAG` instead of treating it as missing OS support — its blanket
skip is what let a never-working path ship.

Verified on this machine (Windows 11 Pro 26100): the client opens, `poll_frame` returns a
real 3840-byte PCM period, and `support(DeviceKind::ProcessLoopback)` now answers
`Supported`. Reverting only the stream-flag line reproduces `0x88890021` in both tests.

## References

- Facade ADR-0001 · live-recorder / sound_capture (reference only)
- [`AUDIOCLIENT_ACTIVATION_PARAMS`](https://learn.microsoft.com/en-us/windows/win32/api/audioclientactivationparams/ns-audioclientactivationparams-audioclient_activation_params)
  · [Application Loopback API sample](https://github.com/microsoft/Windows-classic-samples/tree/main/Samples/ApplicationLoopback)
  (MIT) — both process-loopback modes and the mandatory `AUDCLNT_STREAMFLAGS_LOOPBACK`
