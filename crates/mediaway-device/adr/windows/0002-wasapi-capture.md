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

## Correction (2026-10-13): loopback sound after silence came early

A frame's `pts` is its sample count, and a consumer that lays sound against wall-clock video
(Recoway, `E:\NyxWays\gway\app\recoway`, timestamps audio exactly this way) needs that count
to follow wall time. It did not, for two reasons found by reading `pump_capture_loop` and
then measured on Windows 11 Pro 26300 (Razer headset as the default endpoint, a Virtual
Audio Cable endpoint nothing plays to, and process loopback of the test process):

1. **`AUDCLNT_BUFFERFLAGS_SILENT` packets were dropped without advancing `pts`.** The default
   endpoint, open but playing nothing, sent 150 silent-flagged 10 ms packets in 1.5 s; none
   was delivered or counted.
2. **An idle render endpoint sends no packets at all.** Loopback of the Virtual Audio Cable
   endpoint returned nothing for 1.5 s; the first packet came when a render stream opened,
   flagged `DATA_DISCONTINUITY`, with its device position restarted at 480 and only its QPC
   stamp (1 517 ms after `Start`) recording the gap. The Microsoft
   [loopback recording](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)
   page does not state this; Chromium and OBS both work around it (§ References).

Effect, measured by `lib_tests.rs::loopback_sample_clock_follows_wall_time_through_silence`
on the code before this fix: 3.0 s of capture delivered 1.0 s of samples on both endpoints,
and a tone played at 1.51 s was delivered at sample time 0.02 s — 1.49 s early.

**Decision: deliver silence, and keep loopback's sample index on the QPC clock.**

- Silent-flagged packets are delivered as zeros, for every source (microphones included).
- For loopback (endpoint and process), a sans-IO [`wasapi_timeline.rs`](../../src/windows_audio/wasapi_timeline.rs)
  anchors sample 0 at the QPC time of `Start` and compares each packet's `GetBuffer` QPC stamp
  with where the next sample belongs: a gap over 4 ms is filled with zeros; while no packet is
  waiting, zeros are filled up to 100 ms behind "now" in pieces of at least 20 ms; a packet
  overlapping filled zeros loses its overlapping head. Each stamped packet re-anchors the
  timeline, so device-clock drift never turns into inserted silence.
- A stamp more than 500 ms off the QPC read beside it turns filling off for the session
  (Chromium's rule) rather than inserting bogus silence.
- Zeros come from one static 64 KiB buffer (`Bytes::from_static`): no allocation or copy.

| Alternative | Why not |
|-------------|---------|
| Device position (`pu64DevicePosition`) instead of QPC | Measured useless for both gaps: it restarts after an idle stretch on endpoint loopback and is always 0 on process loopback. |
| A silent render stream on the endpoint (OBS `ClearBuffer`, Chromium's loopback render client) so packets never stop | Keeps the endpoint's audio engine running for the whole capture whether or not anything plays (an energy cost no one measured here), only covers endpoint loopback, and still needs the silent-packet fix. QPC filling covers both sources with no extra stream. Revisit if a driver is found whose idle QPC stamps are unusable. |
| Fill only when the next packet arrives (no live idle fill) | A consumer that cuts the stream by wall time (Recoway's pause drops samples polled while paused) would see the whole idle stretch arrive after the cut. |
| Same wall-clock filling for microphones | Not needed for the reported defect, and OBS defaults device timing off for inputs; left as counting only. |

Trade-offs stated: idle silence trails the wall clock by up to ~120 ms
(`IDLE_LAG_HNS` + one piece), so a wall-clock cut through silence is off by up to that much
silence; endpoint-loopback stamps are presentation times (measured 25-30 ms ahead of the
read), so sample 0 maps to the moment `Start`'s audio would be heard, which is what aligns
with a picture shown at the same moment; drift between the device clock and QPC while sound
plays continuously is unchanged (needs resampling, out of scope).

Verified after the fix: the same test delivers 2.91 s of 3.00 s (idle cable endpoint; the
0.1 s short is the idle lag) and 3.03 s of 3.00 s (default endpoint; the leading presentation
offset), with the tone's onset 41 ms and 52 ms after it was written — render latency. The
timeline's rules are pinned by `wasapi_timeline_tests.rs` (13 tests, no hardware).

## References

- Facade ADR-0001 · live-recorder / sound_capture (reference only)
- [`AUDIOCLIENT_ACTIVATION_PARAMS`](https://learn.microsoft.com/en-us/windows/win32/api/audioclientactivationparams/ns-audioclientactivationparams-audioclient_activation_params)
  · [Application Loopback API sample](https://github.com/microsoft/Windows-classic-samples/tree/main/Samples/ApplicationLoopback)
  (MIT) — both process-loopback modes and the mandatory `AUDCLNT_STREAMFLAGS_LOOPBACK`
- OBS Studio [`plugins/win-wasapi/win-wasapi.cpp`](https://github.com/obsproject/obs-studio/blob/master/plugins/win-wasapi/win-wasapi.cpp)
  (GPL-2.0-or-later — ideas only, no code): `ClearBuffer` keeps endpoint loopback running
  with a silent render stream; loopback audio is stamped with `GetBuffer`'s QPC time
  (`OPT_USE_DEVICE_TIMING` defaults to on for output devices, off for inputs)
- Chromium [`media/audio/win/audio_low_latency_input_win.cc`](https://source.chromium.org/chromium/chromium/src/+/main:media/audio/win/audio_low_latency_input_win.cc)
  (BSD-3-Clause — ideas only): `PushSilence` for `AUDCLNT_BUFFERFLAGS_SILENT`; a separate
  event-driven render client so endpoint loopback runs while nothing plays; QPC stamps more
  than 500 ms off the wall clock replaced by "fake" timestamps (crbug.com/1439283)
