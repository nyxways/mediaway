# ADR-0004: `AudioPlayback` streaming trait

- **Status**: Accepted
- **Date**: 2026-07-31
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device`

## Context

[`AudioCapture`](../src/audio.rs) (ADR-0001) only covers the *input* direction —
microphone, render loopback, process loopback. There is no output/playback
surface anywhere in the workspace: no way to hand PCM frames (e.g. decoder
output) to a real audio device for the user to hear.

Capture and playback share the same underlying OS primitive on Windows
(`IAudioClient`) but have an inverted data direction and, critically, an
inverted real-time obligation:

- **Capture**: the OS produces samples on its own schedule; the app may be
  slow, and the existing contract (ADR-0002, `mediaway-device-windows`)
  already accepts *dropping the oldest* queued samples under backpressure —
  losing some historical mic audio is a tolerable failure mode.
- **Playback**: the OS *consumes* samples on its own schedule (the render
  endpoint's buffer must be kept fed or the audio engine plays silence /
  glitches). If the app is slow, silently dropping newly-submitted audio
  would corrupt playback order and audibly glitch — a materially worse
  failure than capture's drop-oldest, and the trait must decide what happens
  on both an empty queue (underrun) and a full queue (submission
  backpressure) rather than reuse capture's answer unexamined.

The user has explicitly chosen a **native platform-backend** approach
(WASAPI render endpoint on Windows, mirroring `WindowsWasapiCapture`) over a
cross-platform crate such as [`cpal`](https://crates.io/crates/cpal), to stay
consistent with how capture was built and to avoid a new external dependency
that would need [`docs/conventions/deps-policy.md`](../../../docs/conventions/deps-policy.md)
justification for a capability the workspace can already reach with the
`windows` crate alone (same COM/`IAudioClient` foundation already linked for
capture).

## Decision

> Add a facade **`AudioPlayback`** trait — **push model**, not a callback —
> with a bounded internal queue owned by the platform backend, mirroring
> [`AudioCapture`](../src/audio.rs)'s worker-thread + bounded-queue shape in
> the opposite data direction.

### Why push, not callback

WASAPI render is not an OS-driven callback like CoreAudio's render proc. The
documented pattern is: a dedicated thread either polls `IAudioClient::
GetCurrentPadding` on a timer, or waits on an event registered via
`SetEventHandle`, and *itself* calls `IAudioRenderClient::GetBuffer` /
`ReleaseBuffer` — the OS never calls into arbitrary app code on its own
thread. Mediaway already owns that thread shape for capture
(`run_wasapi_worker` / `pump_capture_loop` in
[`wasapi.rs`](../../mediaway-device-windows/src/wasapi.rs)). A callback-style
trait (`AudioPlayback::open(..., on_need_data: impl FnMut(&mut [u8]))`) would:

- Require the trait to be generic or `Box<dyn FnMut>` on a hot path (ZCA
  conflict, [`zero-cost-abstractions.md`](../../../docs/spec/zero-cost-abstractions.md)).
- Force every future backend (Web Audio, CoreAudio, PipeWire) into the same
  closure-ownership shape even where their natural idiom differs.
- Complicate the C-FFI story (`mediaway-device-ffi`, deferred but anticipated
  per [`c-ffi.md`](../../../docs/spec/c-ffi.md)) — a poll/push API maps onto a
  C ABI far more directly than a Rust closure callback would.

A **push** trait — caller calls `write_frame`, backend worker thread drains
an internal queue into the render buffer on its own schedule — keeps the
event/poll loop entirely inside the platform backend (same boundary as
capture, ADR-0002 facade/platform split) and gives callers a synchronous,
non-async, streaming-first shape consistent with
[`async-and-streaming.md`](../../../docs/spec/async-and-streaming.md).

### Public surface

```rust
pub struct AudioPlaybackConfig {
    /// Render endpoint index (`0` = default console render). Mirrors
    /// `AudioCaptureSource::Loopback { device_index }`.
    pub device_index: u32,
    /// Preferred PCM format when conversion is required (`F32` matches modern WASAPI mix).
    pub sample_format: SampleFormat,
}

pub trait AudioPlayback {
    /// Negotiated stream format — real `sample_rate`/`channels`/`format`
    /// after opening (the endpoint's shared-mode mix format on Windows).
    /// `write_frame` payloads must match this exactly; there is no
    /// implicit resample in v1 (see Deferred).
    fn stream_info(&self) -> &StreamInfo;

    /// Enqueue `frame` for playback (FIFO submission order, no PTS-driven
    /// scheduling in v1 — see Deferred). Ownership transfers on success.
    ///
    /// # Errors
    /// - `PlaybackError::InvalidInput` if `frame`'s format doesn't match
    ///   `stream_info()`.
    /// - `PlaybackError::QueueFull(frame)` if the internal bounded queue is
    ///   full — `frame` is handed back unconsumed (mirrors
    ///   `std::sync::mpsc::SyncSender::try_send`'s `TrySendError<T>`), so the
    ///   caller decides: retry, throttle, or drop. No frame is ever silently
    ///   dropped by the backend on the submission side.
    fn write_frame(&mut self, frame: AudioFrame) -> Result<(), PlaybackError>;

    /// Cumulative count of render periods that included any silence
    /// substituted for missing queued audio, since `open`. Never resets.
    /// A poll-based counter, not an error — playback keeps running through
    /// underrun (see Buffering below), it does not abort.
    fn underrun_count(&self) -> u64;

    /// Stop immediately and free OS resources. **Does not drain** the
    /// internal queue — any buffered-but-unplayed frames are discarded
    /// (blocking `flush`-before-close is a deferred addition, not v1).
    /// Blocks until the backend worker thread has stopped touching the
    /// device (mirrors `WindowsWasapiCapture::close`'s join).
    ///
    /// # Errors
    /// Returns [`PlaybackError`] on backend failure.
    fn close(&mut self) -> Result<(), PlaybackError>;
}
```

`AudioPlaybackConfig` deliberately has **no `AudioPlaybackSink` enum** and
**no `time_base`** field, unlike `AudioCaptureConfig` — see Alternatives.

### Buffering / underrun contract

- **Submission side (queue full)**: `write_frame` returns
  `PlaybackError::QueueFull(frame)` — non-blocking, frame returned to caller.
  No silent drop.
- **Render side (queue empty / underrun)**: the backend worker substitutes
  silence for the missing span so playback keeps running, and increments
  `underrun_count`. It does not stop the session and does not return an
  error from `close`/`write_frame` for this — underrun is expected under
  real-time pressure and must be observable, not fatal.
- **`close()` does not drain**: buffered frames are discarded, not played
  out. A drain/flush variant is deferred.

### Error type

New **`PlaybackError`** in [`error.rs`](../src/error.rs), structurally
mirroring `CaptureError` (`Unsupported`, `NoBackend`, `InvalidInput`,
`Backend`, `Closed`, `AccessDenied`) plus one playback-specific variant:
`QueueFull(AudioFrame)`. See Alternatives for why `CaptureError` is not
reused.

### Device selection (v1)

`device_index: u32` (`0` = default console render endpoint via
`IMMDeviceEnumerator::GetDefaultAudioEndpoint(eRender, eConsole)` — the same
enumerator call already used for `AudioCaptureSource::Loopback` in
[`wasapi.rs`](../../mediaway-device-windows/src/wasapi.rs)). Windows v1 only
accepts index `0` (`PlaybackError::Unsupported` otherwise), mirroring the
same restriction already in place for `Microphone`/`Loopback` capture.
Multi-endpoint selection needs real enumeration
(`IMMDeviceEnumerator::EnumAudioEndpoints`, already used read-only in
`capabilities.rs`) — deferred, not needed for a first pass.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| **`cpal`** (cross-platform playback crate) | User-directed rejection: stays consistent with capture's native-backend precedent (ADR-0002, `mediaway-device-windows`), avoids a new external dependency review under [`deps-policy.md`](../../../docs/conventions/deps-policy.md) for a capability the `windows` crate (already linked) can reach directly, and keeps the license/deps graph identical to capture. |
| Callback-driven trait (`FnMut` data-request closure) | Does not match WASAPI's actual poll/event-driven `IAudioClient` design (no true OS render callback thread to hang a closure off of); would force `Box<dyn FnMut>` or generics onto the trait (ZCA / object-safety friction) for no WASAPI-side benefit. |
| Reuse `CaptureError` for playback | `CaptureError`'s `#[error(...)]` messages are literally about capture ("capture backend failure") — reusing them for a real playback failure would be an actively misleading message, violating the honesty bar in [`caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md). `QueueFull(AudioFrame)` also has no capture analog (capture never rejects — it silently drops-oldest instead). |
| `AudioPlaybackSink` enum mirroring `AudioCaptureSource` | `AudioCaptureSource` encodes genuinely different *source kinds* (mic vs render-loopback vs process-loopback). Playback v1 has exactly one kind — a plain render endpoint — so an enum with a single variant is premature abstraction ("no abstractions for one-off code"). Revisit if a second sink kind (e.g. a virtual/loopback-injection sink) is ever needed. |
| `time_base` field on `AudioPlaybackConfig` | Capture uses `time_base` to timestamp frames it produces. Playback *consumes* already-timestamped `AudioFrame`s from the caller and does no PTS-driven scheduling in v1 (FIFO submission order) — an unused field would be dead weight. |
| Drop-oldest on queue-full (mirror capture exactly) | Acceptable for capture (losing old mic history), not for playback: dropping a *newly submitted* frame to make room would silently reorder/skip audio the caller is actively trying to play — an audible correctness bug, not a graceful degradation. Backpressure (`QueueFull`, frame returned) is honest instead. |

## Consequences

### Positive

- Symmetric, low-surprise API for anyone who already knows `AudioCapture`.
- `write_frame`'s push into the queue costs **no additional payload copy**
  beyond whatever the caller already had: `AudioFrame::data` is
  `mediaway_common::Bytes` (`bytes::Bytes`, refcounted) — moving the frame
  into the queue only bumps a refcount. The one unavoidable payload copy is
  symmetric with capture's: the backend worker copying queued bytes into the
  OS-owned render buffer (`IAudioRenderClient::GetBuffer`), which cannot be
  avoided for the same buffer-lifetime reason ADR-0002's capture addendum
  already documents for `IAudioCaptureClient::GetBuffer`.
- `QueueFull` returning the frame gives callers real backpressure instead of
  silent audio loss.

### Negative / Trade-offs

- No implicit resampling: a caller whose `AudioFrame` format doesn't match
  `stream_info()` gets `PlaybackError::InvalidInput`, not silent
  conversion or truncation. Callers must resample themselves before
  `write_frame` if their source format differs from the negotiated endpoint
  format.
- No PTS-driven scheduling: frames play in FIFO submission order at
  whatever rate the caller submits them; av-sync is the caller's
  responsibility in v1.
- `close()` discarding buffered-but-unplayed audio may surprise callers
  expecting the tail of a track to finish playing; documented, not silent.

## Deferred (out of scope for v1)

- Exclusive-mode WASAPI (lower latency, format-locked) — shared mode only
  for v1, mirroring capture's scope.
- Multi-endpoint enumeration/selection beyond `device_index == 0`.
- On-the-fly resampling / format conversion inside the backend.
- PTS-aware scheduling / av-sync.
- Drain/flush-before-close variant.
- Event-driven (`SetEventHandle`) low-latency render loop — see
  `mediaway-device-windows` ADR-0005 for the v1 timer-poll choice and why
  this is deferred rather than adopted immediately.
- `mediaway-device-ffi` C ABI surface for playback (device FFI today only
  covers capture per [`ffi-c-abi.md`](../../../docs/ai/wiki/device/ffi-c-abi.md)).

## References

- [ADR-0001](0001-capture-traits.md) — `AudioCapture`/`VideoCapture` shape this mirrors
- [ADR-0002](0002-facade-platform-boundary.md) — facade/platform split this follows
- [ADR-0003](0003-capability-and-permission-probe.md) — capability probe precedent
- `mediaway-device-windows` ADR-0002 (`wasapi-capture`) — mirrored copy/queue rationale
- `mediaway-device-windows` ADR-0005 (`wasapi-playback`, this pair's backend ADR)
- [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md)
- [`docs/spec/async-and-streaming.md`](../../../docs/spec/async-and-streaming.md)
- [`docs/conventions/deps-policy.md`](../../../docs/conventions/deps-policy.md) — `cpal` rejection
- [`docs/ai/wiki/zero-copy/marks.md`](../../../docs/ai/wiki/zero-copy/marks.md)

ADRs are **English**. Numbering is local to this `adr/` folder.
