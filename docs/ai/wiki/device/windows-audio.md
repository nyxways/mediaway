# Windows audio (WASAPI)

## Capture

- Crate: `mediaway-device-windows` — `WindowsWasapiCapture`
- Sources: mic · system loopback · process loopback
- Format: IEEE float (process loopback fixed 48 kHz stereo)
- Mark today: **🆗** (PCM copied into queue — not CPU ⚡ yet)
- Evaluated shared-buffer CPU ⚡: not achievable under the current `AudioCapture` contract
  (no frame-release hook) + WASAPI's `GetBuffer`/`ReleaseBuffer` lifetime rule — see the
  ADR addendum below. Collapsed the per-period copy from zero-init + memcpy to one write.
- Earn **⚡** with shared/borrowed buffers — [marks](../zero-copy/marks.md)
- ADR: [0002](../../../crates/mediaway-device-windows/adr/0002-wasapi-capture.md)

## Playback

- Facade: `mediaway-device` — `AudioPlayback` / `AudioPlaybackConfig` / `PlaybackError`
  ([ADR-0004](../../../crates/mediaway-device/adr/0004-audio-playback-traits.md))
- Crate: `mediaway-device-windows` — `WindowsWasapiPlayback`
  ([ADR-0005](../../../crates/mediaway-device-windows/adr/0005-wasapi-playback.md))
- Direction is the mirror of capture: **push** model (`write_frame`), not a callback —
  WASAPI render has no true OS-driven callback thread to hang one off. A worker thread
  owns `IAudioClient` + `IAudioRenderClient`; the caller's thread only pushes into a
  bounded `Arc<Mutex<VecDeque<AudioFrame>>>` queue.
- Endpoint: `device_index == 0` only (default render, `GetDefaultAudioEndpoint(eRender,
  eConsole)`); shared mode only; IEEE float mix format only (reuses `wasapi.rs`'s
  `read_float_mix` check as-is). No resampling — format mismatch is `InvalidInput`.
- Worker loop: **timer-poll**, not event-driven — reuses `wasapi.rs`'s proven
  `GetCurrentPadding` + `sleep` shape instead of introducing a second, unverified
  `SetEventHandle`/`WaitForSingleObject` worker shape. Honest trade-off: more
  underrun-prone under load than event-driven render (documented in ADR-0005, not hidden).
- Backpressure is asymmetric with capture: a full submission queue returns
  `PlaybackError::QueueFull(AudioFrame)` (frame handed back, caller decides) instead of
  capture's drop-oldest — dropping a *newly submitted* playback frame would silently
  reorder/skip audio, an audible correctness bug capture's drop-oldest doesn't have.
- Underrun (queue empty on the render side) is not an error: full-period underrun uses
  WASAPI's native `AUDCLNT_BUFFERFLAGS_SILENT`; partial underrun copies what's queued and
  explicitly zero-fills the tail (`SILENT` can't express a partial packet).
  `underrun_count()` increments once per period that hit either case.
- `close()` does **not** drain the queue — buffered-but-unplayed frames are discarded, not
  played out — then blocks on `JoinHandle::join()`.
- Mark: **🆗**, same root cause as capture — `IAudioRenderClient::GetBuffer`'s pointer is
  only valid until `ReleaseBuffer`, so queued PCM is copied into the render buffer once
  per period. `write_frame` itself costs no extra copy (`AudioFrame::data` is
  `bytes::Bytes`, refcounted).
- Hardware-verified: opened a real default render endpoint with an
  empty write queue and observed `underrun_count()` climb via the timer-poll silence-fill
  path — `AUDCLNT_BUFFERFLAGS_SILENT` every period, so no audible output was produced.
- Deferred (see ADR-0004/ADR-0005): exclusive mode, multi-endpoint selection, resampling,
  PTS-aware scheduling, drain-before-close, event-driven render, FFI surface.
