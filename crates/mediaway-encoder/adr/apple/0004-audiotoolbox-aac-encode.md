# ADR-0004: `AudioToolbox` `AudioConverter` AAC encode

- **Status**: Accepted
- **Date**: 2026-08-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (module `mediaway-encoder::apple`)

## ⚠️ Zero real-hardware / zero compile verification in this session

Same structural constraint as every other Apple ADR this session. Every API name/signature cited
below is a direct read of the locally cloned [`objc2`](https://github.com/madsmtm/objc2) checkout
(`local/vendor-ref/objc2/generated/AudioToolbox/AudioConverter.rs`,
`local/vendor-ref/objc2/generated/CoreAudioTypes/CoreAudioBaseTypes.rs`), not a paraphrase from
memory or web search — except where explicitly marked as "public reference usage" below (the
starvation-callback contract's exact semantics *are* confirmed from the local doc comment; the
convention of leaving most destination-`AudioStreamBasicDescription` fields zeroed for a
compressed format is *not* spelled out character-for-character locally, but matches every public
`AudioConverter`+AAC reference implementation found in prior general knowledge — Apple's own
sample code shape, and is flagged here rather than silently presented as locally-grounded).

## Context

`CodecKind::Aac` has exactly one existing backend in this workspace: `mediaway-encoder::windows::
wmf::aac::WmfAacEncoder` (inbox WMF AAC MFT). No Apple AAC backend, and no AAC decoder anywhere at
all (Windows included) exist before this ADR. `AudioEncoderConfig` (`codec`, `sample_rate`,
`channels`, `sample_format`, `time_base`, `bitrate_bps`) is already codec-generic — no
`mediaway-common`/facade change needed.

### `AudioConverter` is pull-based — a genuinely different shape from `VideoToolbox`

`VideoToolbox`'s `VTCompressionSession`/`VTDecompressionSession` are push-based: the app calls
`encode_frame`/`decode_frame`, output arrives later via an async callback VideoToolbox invokes on
its own thread. `AudioConverterFillComplexBuffer` (confirmed, `AudioConverter.rs`) is the
opposite: the app calls it *synchronously*, and it invokes an app-supplied
`AudioConverterComplexInputDataProc` callback **zero or more times, on the calling thread**,
pulling input until either the requested output capacity is filled or the callback signals
starvation. This callback's own doc comment (confirmed, quoted in full below) states the exact
contract this ADR relies on:

> "If the callback returns an error, it must return zero packets of data. `AudioConverterFillComplexBuffer`
> will stop producing output and return whatever output has already been produced to its caller,
> along with the error code. This mechanism can be used when an input proc has temporarily run out
> of data, but has not yet reached end of stream."
>
> "The callback ... is responsible for not freeing or altering this buffer until it is called
> again."

Two consequences this ADR relies on directly:

1. **No cross-thread synchronization needed at all** — unlike every `VideoToolbox` ADR this
   session wrote, the callback runs synchronously on the same thread that called
   `AudioConverterFillComplexBuffer`, nested inside that call. A plain `&mut` borrow into
   `AacEncoder`'s own PCM accumulator, passed as the raw `inInputDataProcUserData` pointer, is
   sound for the callback's whole invocation window — no `Arc`/`Mutex`/refcon-lifetime dance.
2. **"Not altering the buffer until called again" bounds the pointer's validity to within one
   `AudioConverterFillComplexBuffer` call** — once that call returns (whether it filled the
   requested output or the callback signaled starvation), this backend is free to mutate/compact
   its PCM accumulator before the next call.

## Decision

> `AacEncoder` accepts **`SampleFormat::F32`-only** PCM input (Stage 1 — see § Scope), source
> `AudioStreamBasicDescription` = interleaved Float32 PCM, destination ASBD = `kAudioFormatMPEG4AAC`
> with only `mSampleRate`/`mFormatID`/`mChannelsPerFrame`/`mFramesPerPacket` set (the rest left
> zeroed — `AudioConverterNew` resolves the codec's real output shape internally, queryable
> afterward via `kAudioConverterCurrentOutputStreamDescription` if ever needed). `push_frame`
> appends interleaved F32 bytes to an internal `Vec<u8>` accumulator (tracked via a `read_pos`
> cursor, compacted between calls) and drains as many complete AAC frames as are ready via a
> `AudioConverterFillComplexBuffer` loop that stops the moment the input callback signals
> starvation. Extradata (the AAC `AudioSpecificConfig`) is read once, after the first successful
> output packet, via `AudioConverterGetProperty(kAudioConverterCompressionMagicCookie)` —
> confirmed the same **raw ASC bytes** shape (not a `WAVEFORMATEX`-wrapped blob like WMF's
> `MF_MT_USER_DATA`) per `AudioConverter.rs`'s own property doc comment, so no
> `asc_from_waveformatex`-style unwrapping is needed on this platform (a genuine simplification
> versus the Windows backend, not an oversight).

### Module layout and `AppleAudioEncoder` wrapper — mirrors `WindowsAudioEncoder` exactly

`mediaway-encoder::windows::WindowsAudioEncoder` already wraps an `AudioBackend { Aac(WmfAacEncoder),
Opus(SwOpusAudioEncoder) }` enum dispatched by `config.codec` (confirmed,
`src/windows/mod.rs`). This ADR adds the identical shape for Apple:

```text
src/apple/audiotoolbox/aac.rs   — NEW. AacEncoder: AudioEncoder impl, AudioConverter session.
src/apple/mod.rs                — CHANGED. AppleAudioEncoder { inner: Option<AudioBackend> },
                                   AudioBackend { Aac(AacEncoder), Opus(SwOpusAudioEncoder) },
                                   dispatched by config.codec — same shape as WindowsAudioEncoder.
```

`SwOpusAudioEncoder` (`mediaway-sw`-backed, already cross-platform, zero `cfg` gates) needs no new
code — this ADR just wires it into the same per-codec dispatch enum Windows already established,
closing the gap where Apple had no audio-encoder facade type at all (`SwOpusAudioEncoder` could
already be constructed directly by an app, but there was no single "give me an Apple audio
encoder for this `CodecKind`" entry point, unlike Windows).

**Not wired into `mediaway::platform`** — `WindowsAudioEncoder` itself isn't either (confirmed:
grepped `crates/mediaway/src/` for it, zero hits); `AppleAudioEncoder` matches that same scope,
not a new gap this ADR introduces.

### Buffer-supply callback — sketch, grounded against the real doc contract above

```rust
struct InputContext<'a> {
    pcm: &'a [u8],       // remaining unconsumed interleaved F32 bytes
    bytes_per_frame: u32, // channels * 4
    buffer: AudioBuffer,  // reused scratch AudioBuffer the callback points at `pcm`
}

unsafe extern "C-unwind" fn input_proc(..., in_user_data: *mut c_void) -> OSStatus {
    let ctx = unsafe { &mut *(in_user_data.cast::<InputContext>()) };
    let available_frames = ctx.pcm.len() as u32 / ctx.bytes_per_frame;
    if available_frames == 0 {
        // starvation: zero packets, non-zero status — see the quoted contract above.
        return STARVATION_SENTINEL;
    }
    let give = requested.min(available_frames);
    // point ctx.buffer.mData at ctx.pcm's start, mDataByteSize = give * bytes_per_frame;
    // write ctx.buffer into *io_data's single AudioBuffer slot; advance nothing here —
    // AacEncoder::push_frame advances `read_pos` only after FillComplexBuffer returns.
}
```

`STARVATION_SENTINEL` is an arbitrary nonzero `OSStatus` this backend picks itself (the callback's
doc comment only requires "an error", not a specific documented code) — chosen as a private
constant, never surfaced to callers (it never escapes `push_frame`'s own loop).

## Scope (this stage)

**In:**

- `SampleFormat::F32` PCM input only — matches `mediaway_common::SampleFormat::F32`'s own
  "IEEE float32 interleaved PCM" shape exactly, so this backend needs zero sample-format
  conversion code (unlike Windows' WMF backend, which downconverts F32→S16 before feeding the
  MFT — a real quality difference in this backend's favor, not just a scope cut).
- Real `AudioSpecificConfig` extradata, raw bytes (no unwrapping needed, see above).
- `AppleAudioEncoder` (Aac/Opus dispatch), mirroring `WindowsAudioEncoder`'s existing shape.

**Out (deferred):**

- `SampleFormat::S16`/`S32` input — would need a conversion step this ADR does not add; return
  `EncodeError::Unsupported` for now, an honest gap rather than a silent lossy convert.
- HE-AAC/HE-AACv2 (SBR/PS) profiles — `AudioConverterNewSpecific` (explicit codec selection) is
  not used; `AudioConverterNew`'s automatic codec resolution is trusted to pick AAC-LC for the
  sample rates this backend targets, matching WMF's own AAC-LC-only scope.
- `mediaway::platform` wiring — matches `WindowsAudioEncoder`'s own current scope, not a gap this
  ADR introduces.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| `AudioQueueServices` (higher-level, `AudioQueue`-based encoding) | Rejected — a heavier API designed around device I/O queues, not a bare data-to-data transcoder; `AudioConverter` is the documented lower-level primitive for exactly this use case and is what every non-UI AAC encode integration in the wild uses. |
| Convert PCM to `S16` before this backend (mirroring WMF) to keep one shared conversion helper across platforms | Rejected — `AudioConverter` accepts Float32 natively; adding a lossy S16 downconvert purely for cross-platform code-sharing would be a real quality regression for no benefit, and this workspace does not currently share PCM-conversion helpers between the Windows and Apple audio backends regardless. |
| A ring buffer (`VecDeque<u8>`) instead of a `Vec<u8>` + cursor for the PCM accumulator | Rejected — the callback needs a **contiguous** pointer into live memory (`AudioBuffer.mData`); `VecDeque`'s internal buffer is not guaranteed contiguous without `make_contiguous`, which would itself require the same kind of periodic compaction a plain `Vec` + cursor already gives for free. |

## Consequences

### Positive

- No cross-thread synchronization primitives needed anywhere in this backend — a simpler
  concurrency story than every `VideoToolbox` backend this session wrote.
- Extradata is real ASC bytes with no unwrapping — simpler than the Windows backend's own
  `asc_from_waveformatex` two-shape parser.
- `AppleAudioEncoder` closes a real facade gap (no single Apple audio-encoder entry point existed
  before, even though `SwOpusAudioEncoder` was already usable directly).

### Negative / Trade-offs

- **Zero compile verification as authored** — carries over unchanged.
- The destination `AudioStreamBasicDescription`'s "leave most fields zeroed" convention is
  informed by public reference usage, not confirmed character-for-character against this local
  `objc2` checkout's doc comments — flagged, not silently presented as fully locally-grounded.
- `S16`/`S32` PCM input unsupported this stage — a real, if narrow, capability gap versus the
  Windows backend.
- **Larger dependency graph than `VideoToolbox`, confirmed via `cargo tree`**: unlike
  `objc2-video-toolbox` (deliberately avoids `objc2`/`objc2-foundation`, see ADR-0001 § Decision),
  `objc2-audio-toolbox`'s `AudioConverter` feature transitively pulls in full `objc2` +
  `objc2-foundation` (confirmed: `cargo tree -p mediaway-encoder --target x86_64-apple-darwin`
  shows `objc2-audio-toolbox → objc2`, `→ objc2-foundation`, even though this backend calls
  nothing but plain C functions) — Cargo features do not cfg-gate `objc2-audio-toolbox`'s own
  internal dependency requirements down to the single feature this ADR enables. A real, disclosed
  graph-size cost this ADR accepts rather than hides, not a "smaller than the whole ecosystem"
  claim like the video backends get to make.

## References

- `mediaway-encoder::windows::wmf::aac::WmfAacEncoder` — the only other AAC encoder in this
  workspace, the structural precedent for `AudioEncoderConfig`/extradata/`AudioEncoder` shape
- `mediaway-decoder` [ADR-apple/0004](../../../mediaway-decoder/adr/apple/0004-audiotoolbox-aac-decode.md) —
  companion decode-direction ADR from the same session
- Local grounding source (read directly): `local/vendor-ref/objc2/generated/AudioToolbox/
  AudioConverter.rs` (`AudioConverterNew`, `AudioConverterFillComplexBuffer`,
  `AudioConverterComplexInputDataProc`'s full doc comment, `AudioConverterGetProperty`,
  `kAudioConverterCompressionMagicCookie`, `kAudioConverterEncodeBitRate`),
  `local/vendor-ref/objc2/generated/CoreAudioTypes/CoreAudioBaseTypes.rs`
  (`AudioStreamBasicDescription`, `AudioBuffer`, `AudioBufferList`, `AudioStreamPacketDescription`,
  `kAudioFormatMPEG4AAC`, `kAudioFormatLinearPCM`, `kAudioFormatFlagIsFloat`/`IsPacked`)
- `README.md` § Codec support — Apple AAC cell: `👻` → `🆗` once implemented

ADRs are written in **English**.
