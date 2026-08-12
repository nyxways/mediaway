# ADR-0002: AAudio (`ndk::audio`) for Android microphone capture

- **Status**: Accepted
- **Date**: 2026-08-12
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device` (module `mediaway-device::android`, see ADR-0001 § Open questions
  for the shared module-shape decision)

## Context

`AudioCapture`/microphone has no Android backend. Both existing backends in this crate — Windows
WASAPI (`adr/windows/0002-wasapi-capture.md`) and Linux PipeWire (`adr/linux/0004-…`) — share the
same shape: a dedicated worker thread pushes PCM into a bounded, drop-oldest
`Arc<Mutex<VecDeque<AudioFrame>>>` queue that `poll_frame` pops from. This ADR evaluates whether
Android's native low-latency audio API, AAudio, fits that same shape.

## Research: `ndk::audio` — real, safe, and genuinely API-26-gated

Verified against the real cloned source (`local/vendor-ref/ndk/ndk/src/audio.rs`, already present
from the encoder work), not from memory or a web summary.

- `ndk::audio` is a real, safe wrapper (`AudioStreamBuilder`, `AudioStream`) over the whole
  `AAudio*` C API — confirmed present, `#![cfg(feature = "audio")]` at the top of the file.
- **The `audio` feature's own dependency graph bakes in API 26**, confirmed straight from
  `ndk/Cargo.toml`: `audio = ["ffi/audio", "api-level-26"]`. This is hard evidence — not Android
  docs folklore — that any use of `ndk::audio` unconditionally pulls in the crate's
  `api-level-26` cfg gate. **This is a real, structural conflict** with
  `mediaway-encoder::android`'s already-shipped `minSdkVersion` floor of **21** (chosen for
  `AMediaCodec`, introduced in API 21) — the two Android backends in this workspace cannot share
  one `minSdkVersion` floor if this crate wants AAudio. See § Open questions.

### API shape reviewed

- `AudioStreamBuilder::new()` (fallible, `AAudio_createStreamBuilder`) → builder methods:
  `.direction(AudioDirection::Input)`, `.format(AudioFormat::PCM_Float | PCM_I16)`,
  `.sample_rate(i32)`, `.channel_count(i32)`, `.performance_mode(AudioPerformanceMode)`,
  `.sharing_mode(AudioSharingMode::Shared)`, `.device_id(i32)` (`0` = unspecified/system default —
  maps directly onto `Select::Default`) → `.open_stream() -> Result<AudioStream>`.
- **Two consumption models, confirmed from the real source's own doc comments:**
  1. **Callback**: `.data_callback(Box<dyn FnMut(&AudioStream, *mut c_void, i32) ->
     AudioCallbackResult + Send>)`, invoked on a **real-time thread AAudio itself owns**. The
     dependency's own doc comment lists explicit, real constraints on that callback: "must NOT …
     allocate memory using, for example, `malloc()` or new … any file operations … any network
     operations … use any mutexes or other synchronization primitives … sleep … stop or close the
     stream … `read()` … `write()`."
  2. **Blocking read**: `unsafe fn AudioStream::read(&self, buffer: *mut c_void, num_frames: i32,
     timeout_nanoseconds: i64) -> Result<u32>` — an ordinary blocking call from a thread we own,
     `unsafe` only because `buffer` validity is the caller's contract (a raw pointer + length),
     with **no** real-time constraint.
- `AudioStream::request_start()`/`request_stop()` are async requests (state transitions through
  `Starting`/`Started`, `Stopping`/`Stopped` — observable via `.state()`/`.wait_for_state_change()`).
  `Drop for AudioStream` calls `AAudioStream_close` and `.unwrap()`s the result — a real detail
  this crate's own wrapper must be aware doesn't return an error path back to the caller.

### Real, load-bearing design tension found this session

Every existing backend in this crate pushes PCM into a `Mutex`-guarded queue from its
capture-driving thread/callback. **Reusing that exact shape inside AAudio's `data_callback` would
violate the dependency's own documented real-time contract** — taking a mutex inside
`data_callback` is explicitly listed as forbidden, since AAudio's own docs warn it can cause an
audible "pop"/glitch or xrun. This is not a hypothetical risk avoided by luck; it's stated
verbatim in the source this session actually read.

## Decision

> Depend on **`ndk` 0.9** with `features = ["audio", "media"]` (`media` reused for the shared
> screen-capture module's `ImageReader`, see ADR-0003; this ADR alone only needs `audio`) as a
> `[target.'cfg(target_os = "android")'.dependencies]` entry, mirroring the encoder's own gating.
>
> Use `ndk::audio`'s **blocking `read()`** path on a dedicated worker thread — **not** the
> `data_callback` model — so the existing `Arc<Mutex<VecDeque<AudioFrame>>>` drop-oldest queue
> shape (`wasapi.rs`/`mic.rs`'s proven pattern) can be reused verbatim without re-deriving a
> lock-free structure this crate has never needed before. The trade-off (documented, not hidden):
> AAudio's `data_callback` model exists specifically to minimize latency by avoiding exactly this
> kind of thread hop + lock; choosing `read()` accepts a real, if likely small, latency/CPU cost
> versus the callback path in exchange for reusing this crate's already-hardened queue shape.
> Revisit with a lock-free SPSC ring buffer only if a real measurement shows the cost matters (no
> real hardware exists to measure this session).

- New module `mediaway-device::android::mic` (`AndroidMicrophoneCapture`, implementing
  `crate::audio::AudioCapture`): `AudioStreamBuilder::new()` →
  `.direction(AudioDirection::Input).format(AudioFormat::PCM_Float).sharing_mode(Shared)
  .performance_mode(LowLatency)` (unset `sample_rate`/`channel_count` — let AAudio pick the
  device's native config, then read back the actual negotiated values via
  `AudioStream::sample_rate()`/`.channel_count()` after `open_stream()`, mirroring PipeWire mic's
  "leave rate/channels unset in the offer, read back what the server actually gave" pattern) →
  `request_start()` → worker thread loop: `unsafe { stream.read(buf.as_mut_ptr().cast(), buf.len()
  as i32, TIMEOUT_NS) }` → push a copied `AudioFrame` into the bounded queue → `poll_frame` pops.
- **Only `AudioCaptureSource::Microphone { select: Select::Default }`** (→
  `AudioStreamBuilder::device_id(0)`, unspecified/system default) this slice — a nonzero
  `Select::Id` returns `CaptureError::Unsupported`. This mirrors both existing mic backends' own
  first-slice restriction. `Loopback`/`ProcessLoopback` are not modeled by AAudio at all on
  Android (no OS concept of render-endpoint loopback capture the way WASAPI has) — permanently
  `Unsupported` on this platform, not merely deferred.
- **Only `SampleFormat::F32`** (→ `AudioFormat::PCM_Float`) — matches
  `AudioCaptureConfig::microphone`'s existing default and Linux mic's own choice; `AudioFormat::
  PCM_I16` exists in the dependency but is left unsupported this slice, same restriction shape
  `wasapi.rs` already applies to its own first cut.
- **Device enumeration is out of scope this slice.** Unlike WASAPI (`IMMDeviceEnumerator`) or
  PipeWire (its own graph), AAudio's `device_id` is an Android-assigned integer sourced from the
  Java-side `android.media.AudioManager.getDevices()` — there is no NDK enumeration API for input
  device IDs. Only `device_id(0)` (system default) is reachable without a JNI round trip into
  `AudioManager`, consistent with restricting to `Select::Default` above.

### Permission: no MediaProjection-style host-app JNI dance needed here

Unlike screen capture (ADR-0003), microphone access is gated by the ordinary Android runtime
permission `android.permission.RECORD_AUDIO` (`ActivityCompat.requestPermissions`/
`registerForActivityResult(RequestPermission())`) — a normal host-app responsibility already
outside every platform backend's scope in this crate (the same category as Windows' "no
proactive consent-dialog API, Settings > Privacy only" gap this crate's `capabilities.md` wiki
page already documents). `open_stream()` itself is the real, costly probe if permission is
missing (expected: a real `AudioError`, likely `Unavailable`, though the exact variant is
unverified — no real device to test against). `capabilities::request_permission(Microphone)` on
Android should follow the same "open a real session, observe the result, then close" shape
Windows WASAPI already uses — not a cheap query.

## ZCA / typestate shape

| Existing precedent | AAudio | Difference |
|---|---|---|
| WASAPI worker thread + `Arc<Mutex<VecDeque<AudioFrame>>>` bounded, drop-oldest queue | Same queue shape, fed by a blocking `read()` loop instead of `IAudioCaptureClient::GetBuffer`/`ReleaseBuffer` | Buffer source differs; consumer-side shape identical, zero new structure |
| PipeWire mic: negotiate format, read back real rate/channels after connect | Same: unset `sample_rate`/`channel_count` in the builder, read back real values via `AudioStream::sample_rate()`/`.channel_count()` after `open_stream()` | Direct API parity, not just a shape analogy |
| No shared-buffer CPU ⚡ achieved yet anywhere in this crate (`wasapi.rs`'s own evaluated-and-rejected attempt) | Same 🆗 mark expected — `read()` still copies into an owned buffer | No regression, no improvement, over existing marks |

No `Box<dyn _>` planned (the `data_callback`'s own `Box<dyn FnMut…>` type is **not** used, since
this ADR picks the blocking `read()` path instead).

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| AAudio `data_callback` model | Rejected — see § Real, load-bearing design tension: the dependency's own docs forbid mutexes/allocation inside it, which this crate's existing queue shape needs. Would require a new lock-free structure this crate has never needed, for a latency win that cannot be measured without real hardware this session. |
| JNI `android.media.AudioRecord` (older Java API) | Portable to a lower `minSdkVersion` (API 3+) than AAudio's 26, but a Java-managed circular buffer requiring per-read JNI marshaling and without AAudio's low-latency guarantees. AAudio is Google's modern recommended native low-latency path and mirrors the encoder ADR's "prefer the modern native session API over an older JNI-bound one" reasoning — named here as the fallback if the minSdk-26 floor (see Open questions) is rejected. |
| Raw `ndk-sys` `AAudio*` FFI directly (skip the safe `ndk::audio` layer) | No reason to — the safe wrapper exists, is well-documented, and needs zero raw `unsafe` beyond the one `unsafe fn read()` call already documented above. |

## Dependency review (`docs/conventions/deps-policy.md`)

`ndk` itself was already reviewed in `mediaway-encoder` ADR-0001 (license, maintenance,
`rust-version`) and is not re-litigated here. What's new to this ADR: the `audio` feature's own
`api-level-26` requirement (see § Research above) is the only cost this ADR adds beyond the
encoder's existing dependency.

## ⚠️ CI verification plan — same posture as the encoder's Android ADR

**Zero `cargo check`/`clippy`/`test` run against this ADR.** Extend the same `android` CI job
(`.github/workflows/ci.yml`) with a `mediaway-device` lint step — see ADR-0001 § CI verification
plan and ADR-0003 § Open questions for the shared `-p <api-level>` flag decision this crate's step
needs (likely `26`, not the encoder's `21`, because of this ADR's own AAudio requirement).

## Open questions (for user confirmation)

1. **`minSdkVersion` floor for `mediaway-device::android` as a whole** — AAudio genuinely needs
   API 26; `mediaway-encoder::android` shipped at API 21. Options: (a) accept a **higher floor
   (26) for the device crate specifically**, differing from the encoder crate — two Android
   backends in one workspace with two different floors is not a contradiction, just two
   independently-scoped decisions; (b) fall back to JNI `AudioRecord` instead of AAudio to keep
   parity with 21, accepting AudioRecord's JNI marshaling cost. **Recommendation: (a)** — AAudio
   is the correct modern API and the encoder's 21 floor was itself just "maximizes compatibility
   for *that* codec path," not a workspace-wide mandate. See ADR-0003 § Open questions — screen
   capture's clean architecture also needs 26, reinforcing this floor.
2. **`read()` timeout value and buffer size** — no real device to tune against; needs a
   reasonable default (e.g. a few periods' worth) chosen once real hardware is available.
3. **`AudioError` → `CaptureError` mapping table** — which `AudioError` variants map to
   `AccessDenied` vs `Backend` vs `DeviceLost` is unverified without a real permission-denied or
   disconnected-device test.

## Decisions confirmed with the user (2026-08-12)

1. **minSdk floor**: **26** for `mediaway-device::android` as a whole — option (a) from this
   ADR's own recommendation, accepted. `mediaway-encoder::android` keeps its own, separately
   scoped 21 floor.
2. **`read()` timeout/buffer size**: kept as originally proposed (`READ_CHUNK_FRAMES = 480`,
   `READ_TIMEOUT_NS = 20ms`) — reasonable first-slice defaults, not separately re-litigated;
   still unverified against real hardware.
3. **`AudioError` → `CaptureError` mapping**: simplified from the open question's proposal —
   `mic.rs`'s `open_stream()` maps every `AudioStreamBuilder`/`open_stream` failure to a single
   `CaptureError::Backend` rather than attempting a fine-grained per-`AudioError`-variant table,
   since no real device exists this session to confirm which variant a permission denial
   actually produces. `capabilities::request_permission(Microphone)` reports
   `PermissionState::Unknown` rather than `Denied`/`Granted` as an honest consequence — see
   `capabilities.rs` doc comment.

## Implementation notes (2026-08-12, written alongside the code)

- `mic.rs` follows this ADR's decision exactly: `AudioStreamBuilder` with `Direction::Input`,
  `AudioFormat::PCM_Float`, `AudioSharingMode::Shared`, `AudioPerformanceMode::LowLatency`,
  `device_id(0)`, then the **blocking `read()`** path on a dedicated worker thread — the
  `data_callback` model is not used anywhere in this crate.
- Real negotiated `sample_rate()`/`channel_count()` are read back from the opened `AudioStream`
  after `open_stream()` succeeds (not assumed), matching this ADR's own design and the
  PipeWire-mic "read back what the server actually gave" precedent it cites.
- `capabilities::support(Microphone)` reports `Support::Supported` unconditionally — AAudio has
  no cheap NDK-level "is a mic present" query (unlike PipeWire's daemon-connect probe), and
  every real Android device ships at least one microphone; documented as a real, different cost
  class from `linux::mic`'s live probe, not a silently-assumed equivalence.

## Consequences

### Positive

- Reuses this crate's already-hardened `Mutex<VecDeque>` queue shape verbatim — no new
  concurrency primitive introduced.
- `ndk::audio` is a real, actively maintained safe wrapper — zero raw `unsafe` FFI needed beyond
  one documented `unsafe fn read()` call.
- Caught a real, dependency-verified minSdk conflict (API 26 baked into `ndk`'s own `Cargo.toml`
  feature graph, not just Android documentation) before it became a build-time surprise.

### Negative / Trade-offs

- **Zero compile verification as authored** — same caveat class as every Android ADR this
  session.
- Chooses the blocking-read path over AAudio's purpose-built low-latency callback model,
  trading some latency for reuse of existing, proven code — an honest, stated trade-off, not a
  silently worse default.
- Device enumeration is out of scope; only the system default input is reachable this slice.
- Real, structural minSdk conflict with the already-shipped encoder backend — cannot be resolved
  without a maintainer decision (see § Open questions #1).

## References

- [`docs/conventions/deps-policy.md`](../../../../docs/conventions/deps-policy.md)
- `adr/windows/0002-wasapi-capture.md` · `adr/linux/0004-pipewire-microphone-capture.md` — the
  queue-shape precedent this ADR reuses
- `mediaway-encoder` `adr/android/0001-ndk-amediacodec-h264-cpu-upload.md` — the minSdk-21
  decision this ADR's § Open questions #1 flags a real conflict with
- [`ndk::audio` docs.rs](https://docs.rs/ndk/latest/ndk/audio/index.html) ·
  [AAudio NDK guide](https://developer.android.com/ndk/guides/audio/aaudio/aaudio) — cloned
  source read this session: `local/vendor-ref/ndk/ndk/src/audio.rs`
