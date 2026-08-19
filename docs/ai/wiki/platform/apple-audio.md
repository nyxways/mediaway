# Apple audio (`AudioToolbox` `AudioConverter`) — AAC-LC + Opus encode/decode

ADRs: [`mediaway-encoder` ADR-apple/0004](../../../../crates/mediaway-encoder/adr/apple/0004-audiotoolbox-aac-encode.md)/[0005](../../../../crates/mediaway-encoder/adr/apple/0005-audiotoolbox-opus-encode.md),
[`mediaway-decoder` ADR-apple/0004](../../../../crates/mediaway-decoder/adr/apple/0004-audiotoolbox-aac-decode.md)/[0005](../../../../crates/mediaway-decoder/adr/apple/0005-audiotoolbox-opus-decode.md)
— all **Accepted, zero compile verification** (same posture as every other Apple backend this
session). First AAC decoder in this workspace at all — Windows only ever had an encoder.

## `AudioConverter` is pull-based — a real shape difference from `VideoToolbox`

`AudioConverterFillComplexBuffer` is **synchronous**: the app calls it, and it invokes an
app-supplied input callback **on the calling thread**, zero or more times, pulling input until
either the requested output is filled or the callback signals starvation (confirmed from
`AudioConverterComplexInputDataProc`'s own doc comment: "if the callback returns an error, it
must return zero packets of data... this mechanism can be used when an input proc has temporarily
run out of data"). Unlike every `VideoToolbox` backend in this workspace, **no cross-thread
synchronization is needed at all** — a plain `&mut` borrow passed as the raw userData pointer is
sound for the callback's whole nested-call window. Both AAC and Opus share this shape unchanged.

## AAC (`audiotoolbox::{AacEncoder, AacDecoder}`)

- `SampleFormat::F32` interleaved PCM only, both directions — matches `AudioConverter`'s native
  shape, no conversion code needed (unlike Windows' WMF backend, F32→S16).
- Extradata: raw `AudioSpecificConfig` via `kAudioConverter{Compression,Decompression}MagicCookie`
  — encode reads it after first output, decode **requires** it non-empty at `open()` (no in-band
  discovery, matches this session's VP9/AV1 video-decode precedent).
- Assumes raw (non-ADTS) AAC packets both directions — a real, undocumented-as-handled gap for
  ADTS-framed callers.
- Frame size is the AAC-LC spec-fixed `1024` samples/channel — a compile-time constant, no query
  needed (contrast Opus below).

## Opus (`audiotoolbox::{OpusEncoder, OpusDecoder}`) — native, replaces the Apple `SwOpusAudioEncoder` default

- Same `AudioConverter` shape as AAC, `kAudioFormatOpus` — confirmed a plain `AudioFormatID`
  ("has no flags"), zero Opus-specific `AudioToolbox` properties exist anywhere in the local
  `objc2` checkout.
- **No magic cookie / config record at all** — Opus is self-describing per-packet (RFC 6716 §3.1's
  TOC byte), matching `windows::wmf::opus::WmfOpusDecoder`'s existing "no `extra_data`" precedent.
- **Frame duration is converter-chosen, not caller-selectable** — unlike AAC's fixed 1024 and
  unlike `SwOpusAudioEncoder`'s `time_base`-as-frame-duration-selector contract. Discovered via
  `AudioConverterGetProperty(kAudioConverterCurrent{Output,Input}StreamDescription)` after `open()`
  (encoder queries its own *output* ASBD; decoder queries the *input* side — querying decode's
  output would just echo back PCM's own fixed `mFramesPerPacket: 1`, useless). A caller needing a
  specific frame size (e.g. WebRTC's 20 ms convention) should use `SwOpusAudioEncoder` directly —
  not removed, just no longer `AppleAudioEncoder`'s dispatch target on Apple.
- `AppleAudioEncoder`'s `AudioBackend::Opus` now wraps `audiotoolbox::OpusEncoder`;
  `crates/mediaway/src/platform.rs`'s Apple `encoder_support`/`decoder_support` Opus branches probe
  this native backend live instead of the software one.

## `AudioBufferList` cannot be built via a struct literal

Upstream's own guard: `AudioBufferList { mNumberBuffers, mBuffers, _this_is_unsized: () }` has a
**private** zero-sized marker field specifically to block external struct-literal construction.
Every backend here builds it via `MaybeUninit<AudioBufferList>::uninit()` + `addr_of_mut!` field
writes instead — sound because `()` has exactly one always-valid (zero-byte) bit pattern, so
`assume_init()` only needs the real-size fields (`mNumberBuffers`, `mBuffers`) actually written.

## Wiring into `mediaway::platform`

Opus **is** wired (`encoder_support`/`decoder_support` probe the native backends live, this
session). AAC is **not** — matches `WindowsAudioEncoder`/`WmfAacEncoder`'s own current scope, not a
new gap this session introduces.

## Not locally re-verified (flagged, not hidden)

- AAC destination ASBD's "leave most fields zeroed" convention for a compressed format.
- AAC output-packet `pts` math assumes no encoder look-ahead/priming delay (unverifiable without
  real hardware).
- Whether a rejected `kAudioConverterEncodeBitRate` `SetProperty` call still yields a working
  session at the format's own default bitrate (assumed, not confirmed) — applies to both codecs.
- Opus's converter-chosen frame duration and whether `AudioConverterNew` even resolves a nonzero
  `mFramesPerPacket` before any packet has been submitted (decode side falls back to a generous
  guess, 5760 samples/120 ms, if the query returns 0).
