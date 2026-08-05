# Windows Opus decode (WMF)

- **Session**: inbox WMF Opus decoder MFT (`CLSID_MSOpusDecoder` / `CMSOpusDecMFT`,
  `{63E17C10-2D43-4C42-8FE3-8D8B63E46A6A}`), Float32 PCM out.
- **Entry**: `mediaway_decoder::windows::WmfOpusDecoder` (+ `OpusDecoderConfig`) —
  implements the facade `AudioDecoder` trait ([ADR-0003](../../../crates/mediaway-decoder/adr/0003-audio-decoder-trait.md))
  and stays usable standalone (low-level first-class, inherent methods kept). `decoder_support(Opus)`
  probes it live.
- **No encoder MFT**: `MFTEnumEx` returns zero Opus encoder results; encode is wired
  through `mediaway-sw` in `WindowsAudioEncoder` (README codec table, Windows row).
- **Verified** (2026-08-03): roundtrip test (SW-encode sine → WMF decode → real PCM);
  ffmpeg-produced `verify.opus.ogg` (4.02 s) decodes to exact sample counts.
- History: module existed as `pub(crate)` (session-only); made public + `audio`
  feature added to `mediaway-decoder` defaults (the `wmf` module no longer requires
  the `video` feature to compile the audio session).
