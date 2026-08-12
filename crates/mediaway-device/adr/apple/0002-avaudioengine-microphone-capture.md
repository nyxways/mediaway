# ADR-0002: `AVAudioEngine` input-node tap via `objc2`, microphone capture (macOS + iOS)

- **Status**: Accepted
- **Date**: 2026-08-12
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device` (module `mediaway-device::apple`, see ADR-0001 § Open questions
  for the shared module-shape/minimum-OS-floor discussion)

## Context

`AudioCapture`/microphone has no Apple backend. Every existing mic backend in this crate — Windows
WASAPI (`adr/windows/0002`), Linux PipeWire (`adr/linux/0004`), Android AAudio
(`adr/android/0002`) — shares the same consumer-side shape: a dedicated worker thread (or, for
AAudio, a blocking `read()` loop chosen specifically to avoid a callback-model conflict) pushes PCM
into a bounded, drop-oldest `Arc<Mutex<VecDeque<AudioFrame>>>` queue that `poll_frame` pops. This
ADR evaluates, as the task requires, the real, undecided design question: session-based capture
(`AVCaptureSession` + `AVCaptureAudioDataOutput`, reusing ADR-0001's camera plumbing conceptually)
vs. a dedicated pure-audio API (`AVAudioEngine`'s `inputNode` + a tap block).

## Research: two real options, read directly from `objc2` source

### Option A — `AVCaptureSession` + `AVCaptureAudioDataOutput`

Confirmed real in `generated/AVFoundation/AVCaptureAudioDataOutput.rs`: same
`setSampleBufferDelegate:queue:` shape as `AVCaptureVideoDataOutput` (ADR-0001), delivering
`CMSampleBuffer`s to a **second** delegate protocol,
`AVCaptureAudioDataOutputSampleBufferDelegate` (`extern_protocol!`, confirmed
`captureOutput:didOutputSampleBuffer:fromConnection:` — same method name/shape as the video
delegate, different protocol type). Would require: a **second** `define_class!` delegate class, a
**second** `AVCaptureSession` + `AVCaptureDeviceInput` (audio device) + `AVCaptureAudioDataOutput`
session lifecycle, and — the real complication — extracting interleaved PCM out of a
`CMSampleBuffer`'s `CMBlockBuffer` (`CMSampleBufferGetDataBuffer` → `CMBlockBuffer::
copy_data_bytes`/`data_pointer`, the same API surface the encoder ADR-0001 needed to add as an
extra, non-default `objc2-core-media` feature: `"CMBlockBuffer"`).

### Option B — `AVAudioEngine.inputNode` + `installTapOnBus:bufferSize:format:block:`

Confirmed real in `generated/AVFAudio/AVAudioEngine.rs` (`inputNode() -> Retained<AVAudioInputNode>`)
and `generated/AVFAudio/AVAudioNode.rs`:

```text
pub type AVAudioNodeTapBlock = block2::Block<'static, fn(NonNull<AVAudioPCMBuffer>, NonNull<AVAudioTime>)>;

pub unsafe fn installTapOnBus_bufferSize_format_block(&self, bus: AVAudioNodeBus,
    buffer_size: AVAudioFrameCount, format: Option<&AVAudioFormat>, tap_block: &AVAudioNodeTapBlock);
```

Doc comment (read in full, not paraphrased from memory): *"CAUTION: This callback may be invoked
on a thread other than the main thread."* — an explicit, real caution, not an assumption. No
`AVCaptureSession`, no second delegate class, no `AVCaptureAudioDataOutputSampleBufferDelegate`
conformance — a **materially smaller** API surface than Option A for a pure-audio-capture need.

### A real, concrete finding that changes this ADR's implementation shape: `AVAudioPCMBuffer` is planar, not interleaved

Confirmed in `generated/AVFAudio/AVAudioBuffer.rs`:

```text
pub unsafe fn floatChannelData(&self) -> *mut NonNull<c_float>;   // array of per-channel pointers
pub unsafe fn int16ChannelData(&self) -> *mut NonNull<i16>;
```

`floatChannelData`/`int16ChannelData` return an **array of one pointer per channel** — i.e.
`AVAudioPCMBuffer`'s native storage is **non-interleaved (planar)** float/int16 audio.
`mediaway-common::AudioFrame::data` is documented, unambiguously, as **"Interleaved sample
bytes."** (`crates/mediaway-common/src/frame.rs`). **This is a real, load-bearing mismatch no
other mic backend in this crate has had to handle** — WASAPI's mix format is already interleaved;
PipeWire's F32LE stream is already interleaved; AAudio's `PCM_Float` blocking `read()` fills an
interleaved buffer. The `AVAudioEngine` tap callback must **interleave N per-channel planar
buffers into one owned interleaved buffer** on every invocation — a real, additional, per-callback
CPU cost this crate's other mic backends do not pay, and must be named honestly per
`docs/spec/caveats-and-clarity.md` (e.g. `interleave_pcm_f32`), not silently folded into a generic
"copy into queue" comment.

## Decision

> Use **Option B — `AVAudioEngine.inputNode` + `installTapOnBus:bufferSize:format:block:`**, not
> the session-based `AVCaptureAudioDataOutput` path. Reasoning: `AVAudioEngine` is purpose-built
> pure-audio-capture, needs no `AVCaptureSession`/device-input/second-delegate-class machinery
> ADR-0001's camera path already carries, and delivers `AVAudioPCMBuffer` (interleaving is the
> only real cost, see above) rather than requiring `CMBlockBuffer` extraction (the encoder's own
> extra, non-default `"CMBlockBuffer"` feature — avoided entirely here). This mirrors the shape of
> this ADR-set's own § "prefer the smaller, purpose-built API over the general session-based one
> when both exist" reasoning, the same class of judgment Android's ADR-0002 used to prefer
> AAudio's blocking `read()` over its `data_callback`.

- New module `mediaway-device::apple::mic` (`AppleMicrophoneCapture`, implementing
  `crate::audio::AudioCapture`): `AVAudioEngine::new()` → `.inputNode()` →
  `.outputFormatForBus(0)` (read the input's **native** format — mirrors PipeWire mic's and
  AAudio's "leave rate/channels unset, read back what the hardware actually gave" pattern, real
  precedent both other backends already established) → `installTapOnBus_bufferSize_format_block`
  with `format: None` (per the doc comment: passing the node's own current format when tapping an
  **input** bus rather than attempting a converting tap) and a `block2::RcBlock`/closure wrapping
  the shared `Arc<Mutex<VecDeque<AudioFrame>>>` → `engine.startAndReturnError` (needs confirming
  exact method name/signature — not read this pass, § Open questions) to begin the audio graph.
- The tap block interleaves `floatChannelData`'s per-channel pointers × `frameLength()` samples
  into one `Bytes` buffer, builds an `AudioFrame`, and pushes it — same drop-oldest bounded queue
  shape every other backend uses.
- **Only `SampleFormat::F32`** this slice, matching every other backend's first-cut restriction
  (`wasapi.rs`, `linux::mic`, `android::mic`) — `int16ChannelData`'s existence is noted but not
  wired up.
- **No `define_class!` needed for this domain at all** — `AVAudioNodeTapBlock` is a plain
  `block2::Block` closure type, not a delegate protocol. This is a genuinely simpler pattern than
  ADR-0001's camera delegate, closer in shape to VideoToolbox's C-callback (a closure/function
  pointer handed to a session-owning object) than to a full Objective-C class definition.

## Authorization

Same `AVCaptureDevice::authorizationStatusForMediaType`/`requestAccessForMediaType_
completionHandler` pair as ADR-0001, called with `AVMediaTypeAudio` instead of `AVMediaTypeVideo`
— confirmed real for both media types in the same `authorizationStatusForMediaType` doc comment
(*"either AVMediaTypeVideo or AVMediaTypeAudio"*). No AAudio-style ambiguity (Android ADR-0002's
"no reliable way to distinguish a RECORD_AUDIO denial from any other failure") — Apple's
authorization API is a real, dedicated, typed status query for both media kinds alike.

## ZCA / typestate shape

| Existing precedent | `AVAudioEngine` tap | Difference |
|---|---|---|
| WASAPI/PipeWire/AAudio: worker thread + `Arc<Mutex<VecDeque<AudioFrame>>>` bounded, drop-oldest queue, fed by a pull (`GetBuffer`/`read()`) | Same queue shape, fed by a **push** callback (`AVAudioNodeTapBlock`) — closer to VideoToolbox's push-callback-into-queue than to any existing mic backend's pull model | First **push**-driven mic backend in this crate — no dedicated capture worker thread needed, `AVAudioEngine` owns its own real-time audio thread internally |
| PipeWire mic / AAudio: negotiate then read back real rate/channels | Same: read `inputNode.outputFormatForBus(0)` after construction, before installing the tap | Direct API parity, not just a shape analogy |
| Every other backend: PCM already interleaved at the source | **Must interleave** planar `floatChannelData` per callback — a real, new, named cost this domain alone pays | See § "A real, concrete finding" above |

No `Box<dyn _>` planned; the tap closure is wrapped via `block2`'s `RcBlock`/`Block` construction
helpers (exact constructor API not pinned down this pass — § Open questions), capturing the shared
`Arc` by move, same ownership shape as every other callback-fed backend in this crate.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| `AVCaptureSession` + `AVCaptureAudioDataOutput` (Option A) | Rejected — see § Decision. Materially larger API surface (second `AVCaptureSession`, second `define_class!` delegate, `CMBlockBuffer` extraction) for no benefit over `AVAudioEngine` for a pure-microphone-capture need. Would make sense only if this crate later wants **synchronized** audio+video capture from one session (`AVCaptureDataOutputSynchronizer`), out of scope this stage. |
| `AudioUnit`/`AudioComponent` (lower-level Core Audio, pre-`AVAudioEngine`) | Older, C-API-shaped, no known `objc2`-ecosystem safe wrapper reviewed this pass; `AVAudioEngine` is Apple's current recommended high-level audio-graph API and already the chosen path — no reason to drop to a lower layer for a first slice. |
| `AVAudioRecorder` (file-based recording API) | Writes to a file/URL by design, not a streaming PCM-buffer API — wrong shape for `AudioCapture::poll_frame`. |

## Dependency review (`docs/conventions/deps-policy.md`)

| Question | Answer |
|----------|--------|
| Need | Real — `AVAudioEngine` is the modern, recommended low-latency audio-graph API on Apple platforms; no lighter safe wrapper exists in the reviewed `objc2` ecosystem. |
| New surface vs. ADR-0001 | `objc2-avf-audio` (the framework crate backing `AVFAudio`/`AVFoundation`'s `AVAudio*` symbols — confirmed named `objc2-avf-audio` in `objc2-av-foundation`'s own `Cargo.toml` dependency list, **not** `objc2-av-f-audio`) is new; `block2` is shared with ADR-0001's authorization path (not an incremental new dependency beyond that). |
| License | Same `Zlib OR Apache-2.0 OR MIT` workspace line, same `madsmtm/objc2` monorepo — not re-litigated. |
| Alternatives | See § Alternatives Considered. |

## ⚠️ CI verification plan

Same as ADR-0001 — reuse the encoder's existing `apple-macos`/`apple-ios` CI jobs, extended with a
`mediaway-device` step, not a new job. **Zero compile verification as authored.**

## Open questions (for user confirmation)

1. **`AVAudioEngine::startAndReturnError` exact signature** — not read this pass (only
   `inputNode`/`installTapOnBus...` were confirmed); needs pinning down before implementation
   (likely a `Result`-returning method per Cocoa's `NSError**` out-parameter convention, mirroring
   `deviceInputWithDevice_error` and every other `..._error` method already confirmed in ADR-0001).
2. **`block2` closure-construction API** — whether `AVAudioNodeTapBlock`'s `block2::Block<'static,
   fn(...)>` is constructed via `block2::RcBlock::new`, a `#[block2::block]` proc-macro-style
   helper, or a different entry point was not confirmed this pass; needs a real read of `block2`'s
   own crate docs/examples before implementation.
3. **Interleaving cost — buffer size / allocation strategy**: whether to allocate a fresh `Bytes`
   per callback (simplest, matches every other backend's per-frame allocation) or reuse a
   preallocated scratch buffer given this callback may run on a real-time-sensitive thread (the
   doc comment's "may be invoked on a thread other than the main thread" caution, though not as
   strict a real-time contract as AAudio's `data_callback` forbidding allocation entirely —
   `AVAudioNodeTapBlock`'s doc comment does not carry that same explicit prohibition).
4. **Minimum OS floor** — shared with ADR-0001 § Open questions #1; `AVAudioEngine`/`inputNode`
   are believed usable on OS versions at least as old as `AVCaptureVideoDataOutput`'s floor
   (**not confirmed this pass** — same local-grounding limitation as ADR-0001).
5. **Device selection**: `AVAudioEngine.inputNode` uses the **system default** input by design —
   whether per-device selection needs a lower-level `AVAudioSession`
   (`setPreferredInput:error:`, iOS-only concept) is out of scope unless the user wants
   non-default mic selection in this first slice (every other backend in this crate restricts to
   `Select::Default` in its first pass too — same restriction shape proposed here).

## Decisions confirmed with the user (2026-08-12)

1. **`SCStreamOutputType::Microphone` (ADR-0003's own open question) — keep separate**: confirmed
   `AppleMicrophoneCapture` (this ADR's `AVAudioEngine` design) stays **the** microphone backend
   on both macOS and iOS. `ScreenCaptureKit`'s `SCStreamOutputType::Microphone` output type
   (ADR-0003 § Research "A real, notable finding") is explicitly **not** used this pass — macOS
   screen capture does not fold mic audio into its own `SCStream` session. This ADR's design is
   otherwise unchanged; recorded here because the question was raised in ADR-0003 but resolves in
   favor of this ADR's scope.

## Implementation notes (2026-08-13, written alongside the code)

- **Open question #1 resolved**: `AVAudioEngine::startAndReturnError(&self) -> Result<(),
  Retained<NSError>>` — confirmed real signature, matching Cocoa's ordinary `NSError**`
  out-parameter convention every other `..._error` method in this ADR set already uses.
- **Open question #2 resolved**: `block2::RcBlock::new(move |args| { ... })` is the real
  closure-construction entry point (confirmed via `block2`'s own doc example and a real usage
  site in the `objc2` repo's `examples/metal/events/main.rs`) — no separate proc-macro helper
  needed.
- `mic.rs` follows this ADR's decision exactly: `AVAudioEngine::new()` → `.inputNode()` →
  `.outputFormatForBus(0)` (read real negotiated format) →
  `installTapOnBus_bufferSize_format_block(0, buffer_size, None, &tap_block)` →
  `startAndReturnError()`.

## Consequences

### Positive

- Materially smaller API surface than the session-based alternative — no second
  `AVCaptureSession`/delegate class needed, avoiding `CMBlockBuffer` entirely (the one extra,
  non-default `objc2-core-media` feature the encoder ADR had to add).
- First push-callback-driven mic backend in this crate — reuses the same drop-oldest queue shape
  as every existing backend on the consumer side; no new concurrency primitive.
- Found a real, concrete, load-bearing bug-shaped mismatch (`AVAudioPCMBuffer`'s planar layout vs.
  `AudioFrame`'s documented interleaved contract) **before** any code was written — exactly the
  kind of catch this workspace's "clone and read real source" convention exists for.

### Negative / Trade-offs

- **Zero compile verification as authored**, same class as every Apple ADR this session.
- Real, new per-callback interleaving cost this domain alone pays among this crate's mic backends
  — must be named honestly in code (`docs/spec/caveats-and-clarity.md`), not silently absorbed
  into a generic copy comment.
- Two real API-shape unknowns (`startAndReturnError` signature, `block2` closure-construction
  entry point) not resolved by this pass — block implementation start until confirmed.
- `AVAudioEngine.inputNode` is system-default-only by design this slice — no per-device selection
  reachable without a separate `AVAudioSession` dependency this ADR does not add.

## References

- [`docs/conventions/deps-policy.md`](../../../../docs/conventions/deps-policy.md)
- `adr/windows/0002-wasapi-capture.md` · `adr/linux/0004-pipewire-microphone-capture.md` ·
  `adr/android/0002-aaudio-microphone-capture.md` — the queue-shape precedent this ADR reuses, and
  the "blocking/pull vs. callback/push" design-tension precedent (Android's AAudio) this ADR's own
  research question mirrors
- ADR-0001 (this set) — shared authorization API, minimum-OS-floor open question, module shape
- `mediaway-common::AudioFrame` (`crates/mediaway-common/src/frame.rs`) — "Interleaved sample
  bytes" contract this ADR's central finding is measured against
- Local grounding source (read directly, not web-fetched):
  `local/vendor-ref/objc2/generated/AVFAudio/{AVAudioEngine,AVAudioNode,AVAudioBuffer,
  AVAudioFormat}.rs`,
  `local/vendor-ref/objc2/generated/AVFoundation/AVCaptureAudioDataOutput.rs`,
  `local/vendor-ref/objc2/framework-crates/objc2-av-foundation/Cargo.toml` (confirms
  `objc2-avf-audio` dependency name)
- [`objc2` on GitHub](https://github.com/madsmtm/objc2) (`Zlib OR Apache-2.0 OR MIT`)
