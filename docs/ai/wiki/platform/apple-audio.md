# Apple audio (`AudioToolbox` `AudioConverter`) — AAC-LC encode + decode

ADRs: [`mediaway-encoder` ADR-apple/0004](../../../../crates/mediaway-encoder/adr/apple/0004-audiotoolbox-aac-encode.md),
[`mediaway-decoder` ADR-apple/0004](../../../../crates/mediaway-decoder/adr/apple/0004-audiotoolbox-aac-decode.md)
— both **Accepted, zero compile verification** (same posture as every other Apple backend this
session). First AAC decoder in this workspace at all — Windows only ever had an encoder
(`windows::wmf::aac`), never a decoder.

## `AudioConverter` is pull-based — a real shape difference from `VideoToolbox`

`AudioConverterFillComplexBuffer` is **synchronous**: the app calls it, and it invokes an
app-supplied input callback **on the calling thread**, zero or more times, pulling input until
either the requested output is filled or the callback signals starvation (confirmed from
`AudioConverterComplexInputDataProc`'s own doc comment: "if the callback returns an error, it
must return zero packets of data... this mechanism can be used when an input proc has temporarily
run out of data"). Unlike every `VideoToolbox` backend in this workspace, **no cross-thread
synchronization is needed at all** — a plain `&mut` borrow passed as the raw userData pointer is
sound for the callback's whole nested-call window.

## Encode (`mediaway-encoder::apple::audiotoolbox::AacEncoder`)

- Input: `SampleFormat::F32` interleaved PCM only (Stage 1) — matches `AudioConverter`'s native
  Float32 input shape exactly, so **no PCM conversion code is needed** (unlike Windows' WMF
  backend, which downconverts F32→S16 before feeding the MFT — a real quality win here, not just
  a scope cut). `S16`/`S32` input returns `EncodeError::Unsupported`.
  `AppleAudioEncoder { Aac | Opus }` mirrors `windows::WindowsAudioEncoder`'s `AudioBackend` enum
  exactly, closing a real gap (no single "give me an Apple audio encoder" entry point existed
  before, even though `SwOpusAudioEncoder` was already directly constructible).
- Destination `AudioStreamBasicDescription` (`kAudioFormatMPEG4AAC`) leaves most fields zeroed —
  `AudioConverterNew` resolves the codec internally. Informed by public `AudioConverter`+AAC
  reference usage, not locally re-verified character-for-character.
- Extradata: raw `AudioSpecificConfig` bytes via `AudioConverterGetProperty
  (kAudioConverterCompressionMagicCookie)` — **no unwrapping needed**, unlike WMF's
  `asc_from_waveformatex` two-shape parser (`MF_MT_USER_DATA` wraps it in a `WAVEFORMATEX`).
- PCM accumulator: plain `Vec<u8>` + `read_pos` cursor (not `VecDeque`) — the callback needs a
  **contiguous** pointer into live memory for `AudioBuffer.mData`.
- Bitrate (`kAudioConverterEncodeBitRate`) set best-effort — a rejection is not fatal to opening.
- **Larger dependency graph than `VideoToolbox`**: confirmed via `cargo tree` that
  `objc2-audio-toolbox`'s `AudioConverter` feature pulls in full `objc2`/`objc2-foundation`
  (unlike `objc2-video-toolbox`, which deliberately avoids them, ADR-0001) — Cargo features don't
  cfg-gate this crate's own internal dependency requirements that finely.

## Decode (`mediaway-decoder::apple::audiotoolbox::AacDecoder`)

- Output: `SampleFormat::F32` interleaved PCM only — symmetric with the encoder's sole input
  shape, a deliberate round-trip pairing.
- **Requires `AacDecoderConfig::extra_data` (raw ASC) non-empty at `open()`** —
  `AudioConverterSetProperty(kAudioConverterDecompressionMagicCookie)` needs it before any
  decoding can happen; no in-band discovery exists for AAC (unlike H.264's SPS/PPS), matching this
  session's VP9/AV1 video-decode "container must supply the config record" precedent.
- **Assumes raw (non-ADTS) AAC packets** — matches the encoder's own output shape and this
  workspace's MP4/`esds`-first convention. ADTS-framed input is a real, undocumented-as-handled
  gap (would misdecode: the 7-byte ADTS header would be read as payload bytes) — not stripped.
- `AacDecoderConfig` is a new, backend-local config type — no shared `AudioDecoderConfig` exists
  anywhere in this crate (every audio-decode backend defines its own, confirmed while researching
  this: `windows::OpusDecoderConfig` and `mediaway_sw::opus::config::OpusDecoderConfig` are two
  *distinct* types with the same name in different modules).
- No `AppleAudioDecoder` wrapper — mirrors `WmfOpusDecoder`'s own direct-exposure precedent (no
  `WindowsAudioDecoder` wrapper exists either).

## `AudioBufferList` cannot be built via a struct literal

Upstream's own guard: `AudioBufferList { mNumberBuffers, mBuffers, _this_is_unsized: () }` has a
**private** zero-sized marker field specifically to block external struct-literal construction.
Both backends build it via `MaybeUninit<AudioBufferList>::uninit()` + `addr_of_mut!` field
writes instead — sound because `()` has exactly one always-valid (zero-byte) bit pattern, so
`assume_init()` only needs the real-size fields (`mNumberBuffers`, `mBuffers`) actually written.

## Neither wired into `mediaway::platform`

Matches existing precedent exactly — `WindowsAudioEncoder`/`WmfOpusDecoder` aren't wired into the
cross-platform facade either (confirmed via grep before adding these); not a new gap.

## Not locally re-verified (flagged, not hidden)

- Destination ASBD's "leave most fields zeroed" convention for a compressed format.
- Output-packet `pts` math assumes each output packet corresponds to exactly one
  `AAC_FRAME_SAMPLES`-sized (1024) chunk of newly-consumed samples — true absent encoder
  look-ahead/priming delay, unverifiable without real hardware.
- Whether a rejected `kAudioConverterEncodeBitRate` `SetProperty` call still yields a working
  session at the format's own default bitrate (assumed, not confirmed).
