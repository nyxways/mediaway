# Windows audio (WASAPI)

## Capture

- Module: `mediaway-device::windows_audio` — `WindowsWasapiCapture`
- Sources: mic · system loopback · process loopback
- Format: IEEE float (process loopback fixed 48 kHz stereo)
- Mark today: **🆗** (PCM copied into queue — not CPU ⚡ yet)
- **Process loopback (`wasapi_process.rs`) — read before touching it:**
  - `Initialize` takes `AUDCLNT_STREAMFLAGS_LOOPBACK` and **only** that. The
    sample-rate-conversion flags (`AUTOCONVERTPCM`/`SRC_DEFAULT_QUALITY`) make it fail with
    `AUDCLNT_E_INVALID_STREAM_FLAG` (`0x88890021`) — there is no mix format to convert from.
    Shipped wrong from the start; fixed 2026-09-18.
  - Windows has **two** modes and no third: `INCLUDE_TARGET_PROCESS_TREE` (the target + its
    children) and `EXCLUDE_TARGET_PROCESS_TREE` (**everything else**). There is no "this
    process but not its children" — `ProcessTreeScope::ProcessOnly` claimed to be that and
    selected the exclude mode, recording the inverse silently until it was renamed
    `ExcludeProcessTree`. Both mappings are pinned by `wasapi_process_tests.rs`.
  - Failures carry the `HRESULT` via `CaptureError::BackendCode { code }`. Do not go back to
    a bare `CaptureError::Backend` here — the discarded code is exactly what hid the flag bug
    from every caller and test for the module's whole life.
  - Both defects: [ADR-0002 § Correction (2026-09-18)](../../../../crates/mediaway-device/adr/windows/0002-wasapi-capture.md).
- **Sample clock = wall clock for loopback** (`wasapi_timeline.rs`, sans-IO, unit-tested):
  `pts` is the sample index, contiguous, silence delivered as zeros (static zero buffer, no
  alloc). Measured 2026-10-13: an *active* endpoint playing nothing sends
  `AUDCLNT_BUFFERFLAGS_SILENT` packets (used to be dropped uncounted); an *idle* endpoint
  (no render stream) sends **no packets**, then resumes with a QPC stamp seconds later and a
  restarted device position; process loopback sends plain zero packets with device
  position 0. So gaps are filled from **QPC** stamps (never device position), live while
  idle (100 ms behind now), and a packet overlapping filled silence loses its head. Endpoint
  loopback QPC stamps run ~25-30 ms *ahead* of read time (presentation time), process
  loopback ~15 ms behind. Microphones: counted only. Device-vs-QPC drift: not corrected.
  [ADR-0002 § Correction (2026-10-13)](../../../../crates/mediaway-device/adr/windows/0002-wasapi-capture.md)
- Evaluated shared-buffer CPU ⚡: not achievable under the current `AudioCapture` contract
  (no frame-release hook) + WASAPI's `GetBuffer`/`ReleaseBuffer` lifetime rule — see the
  ADR addendum below. Collapsed the per-period copy from zero-init + memcpy to one write.
- Earn **⚡** with shared/borrowed buffers — [marks](../zero-copy/marks.md)
- ADR: [0002](../../../../crates/mediaway-device/adr/windows/0002-wasapi-capture.md)

## Playback

- Facade: `mediaway-device` — `AudioPlayback` / `AudioPlaybackConfig` / `PlaybackError`
  ([ADR-0004](../../../crates/mediaway-device/adr/0004-audio-playback-traits.md))
- Module: `mediaway-device::windows_audio` — `WindowsWasapiPlayback`
  ([ADR-0005](../../../../crates/mediaway-device/adr/windows/0005-wasapi-playback.md))
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
- Underrun (queue empty on the render side) is not an error. Since 2026-10-13 (ADR-0005
  § Updates) the worker releases exactly the queued frames and, with nothing queued, keeps
  only a 20 ms `AUDCLNT_BUFFERFLAGS_SILENT` lead in the buffer; before, it filled the whole
  1 s buffer with silence, so audio written after a quiet stretch started about a second
  late. `underrun_count()` counts the silence releases.
- Clock and flush (inherent, Windows only): `played_frames()` is the caller's PCM heard so
  far, from `IAudioClock::GetPosition` mapped back through the released stretches (inserted
  silence not counted); `flush()` drops queue and render buffer (`Stop`/`Reset`/`Start` on
  the worker) and zeroes the clock. First user: gway's Recoway preview playback, which keeps
  its picture on `played_frames()` (measured there: picture within 20 ms of the clock).
- `close()` does **not** drain the queue — buffered-but-unplayed frames are discarded, not
  played out — then blocks on `JoinHandle::join()`.
- Mark: **🆗**, same root cause as capture — `IAudioRenderClient::GetBuffer`'s pointer is
  only valid until `ReleaseBuffer`, so queued PCM is copied into the render buffer once
  per period. `write_frame` itself costs no extra copy (`AudioFrame::data` is
  `bytes::Bytes`, refcounted).
- Hardware-verified: opened a real default render endpoint with an
  empty write queue and observed `underrun_count()` climb via the timer-poll silence-fill
  path — `AUDCLNT_BUFFERFLAGS_SILENT` every period, so no audible output was produced.
  2026-10-13: 300 ms of zeros written, `played_frames()` reached 200 ms 261 ms later and
  `flush()` set it back to 0 (Recoway's ignored `the_speaker_clock_counts_silence_written_and_flush_resets_it`).
- Deferred (see ADR-0004/ADR-0005): exclusive mode, multi-endpoint selection, resampling,
  PTS-aware scheduling, drain-before-close, event-driven render, FFI surface.
