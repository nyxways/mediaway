# Mediaway release notes

<!-- Accumulate development changes under ## Unreleased

### Fixed

- **Windows (WMF) video timestamps did not survive the encoder.** `to_hns` and `from_hns`
  **both truncated**, so the tick → hns → tick trip through the MFT was not an inverse, and
  10 000 000 does not divide most timebase denominators. The two codecs broke differently
  (`crates/mediaway-encoder/adr/windows/0013-wmf-timestamp-round-trip.md`):

  - **HEVC — presentation timestamps collapsed.** At `1/60` only every third tick survived;
    a real recording held 279 video packets and 215 distinct presentation timestamps, and
    ffmpeg warned *"non monotonically increasing dts to muxer"* while decoding it.
  - **H.264 — B-frames presented before they were decoded** (`dts > pts`): truncation shaved
    off the MFT's one-tick reorder delay.

  `from_hns` now rounds to the nearest tick, in both `mediaway-encoder` and
  `mediaway-decoder` (which carried a byte-identical copy). `1/30`, `1/24`, `1001/30000` and
  audio were affected the same way.

  *Corrected after #100:* that PR also said `dts` had been a copy of `pts`. It had not —
  `drain_output` already assigned a decode-order counter — and a
  `MFSampleExtension_DecodeTimestamp` read it added was dead code, now removed. `Packet::dts`
  behaviour is unchanged by either PR.

- **Windows per-process audio loopback never opened, on any machine.**
  `IAudioClient::Initialize` was passed `AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM |
  AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY` and answered `AUDCLNT_E_INVALID_STREAM_FLAG`
  (`0x88890021`) every time: the process-loopback virtual device requires
  `AUDCLNT_STREAMFLAGS_LOOPBACK` and accepts no sample-rate conversion, because there is no
  mix format to convert from. `DeviceKind::ProcessLoopback` capability probing shares that
  code path, so it reported "not supported" everywhere, Windows 11 included. Both now work
  and are covered by tests that open a real session
  (`crates/mediaway-device/adr/windows/0002-wasapi-capture.md` § Correction).

- **`ProcessTreeScope::ProcessOnly` recorded the inverse of what it promised.** Documented as
  "only audio rendered directly by the target process", it selected
  `PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE` — everything *except* that process
  tree — and did so silently, since the session opened normally. Windows has no
  "target process alone" mode, so the variant is **renamed `ExcludeProcessTree`** and
  documented as the "record everything else" mode it always was. The scope → Windows-mode
  mapping is now pinned by unit tests.

- **HEVC in MP4 decoded zero frames. It now decodes every frame.** Not a regression — no
  HEVC MP4 this workspace ever wrote was playable, on any platform. Three defects had to
  line up, and each had been deferred as somebody else's job
  (`crates/iso-bmff/adr/0006-hevc-in-mp4.md`):

  - `iso_bmff::bitstream::hevc::build_hvcc` read `profile_tier_level()` at fixed byte
    offsets into the NAL **as transmitted**, by analogy to `build_avcc`. The analogy does
    not hold: H.264's profile/level sit where an emulation-prevention escape cannot occur,
    HEVC's span RBSP bytes 3..15, and a Main-profile SPS carries three escapes inside
    exactly that range. Measured against ffmpeg's `hvcC` for the same encoder, **6 of 23
    header bytes were wrong**, `general_level_idc` among them — `0x00` instead of `0x5a`.
    **This also corrupted every HEVC file produced on macOS**, which shares `to_hvcc`.
  - `iso_bmff::Muxer::push_packet` did not convert HEVC Annex-B to length-prefixed samples,
    while `hvcC` declares `lengthSizeMinusOne = 3`. A `00 00 00 01` start code read as a
    length gives `Invalid NAL unit size (17564159 > 22953)`, which is what ffmpeg reported
    for every sample. HEVC now gets the same automatic conversion and configuration-record
    backfill H.264 has always had.
  - `mediaway_encoder`'s WMF path never built an `hvcC` at all, passing Media Foundation's
    raw sequence header through as `extra_data` — the follow-up `mediaway-encoder`'s
    ADR-0010 named and deferred. Worse, the inbox `HEVCVideoExtensionEncoder` MFT never
    publishes that attribute, so `extra_data` was *empty* and the muxer wrote
    `HVCC_PLACEHOLDER`, a record whose `numOfArrays` is 0.

  **Verified end to end, as a test rather than a claim:**
  `crates/iso-bmff/tests/conformance_hevc.rs` encodes real HEVC with ffmpeg, strips it to
  Annex-B, muxes it through this crate with an *empty* configuration record, and asserts
  ffprobe decodes all 30 frames. Both container-side defects were confirmed to fail it
  individually before the fix. It skips loudly without ffmpeg/ffprobe.

  **Why this survived seven weeks:** the HEVC mux test fed the muxer six hand-written bytes,
  `build_hvcc`'s unit test used an SPS fixture containing no escape sequence, and the only
  encode→mux→demux integration tests were H.264-only *and* orphaned (no `[[test]]` entry in
  `Cargo.toml`, importing a crate that no longer exists, so cargo never built them).

### Added

- **Windows AAC decode** — `mediaway_decoder::windows::WmfAacDecoder` (+ `AacDecoderConfig`)
  over the inbox `CMSAACDecMFT`, Float32 PCM out, wired into
  `mediaway::platform::decoder_support(Aac)`. Closes a gap where this workspace could
  *encode* AAC into fMP4 on Windows but had no way to play it back. Hardware-verified with
  a sample-exact round trip (4096 PCM samples/channel → 4 AAC-LC packets → 4096 samples
  back, real signal not silence), moving the README's Windows AAC decode cell `👻 → ✅`
  (`crates/mediaway-decoder/adr/windows/0006-wmf-aac-decode.md`).

  The stream's `AudioSpecificConfig` is **required** at open (`extra_data`, e.g. MP4's
  `esds` DecoderSpecificInfo); an empty one is `DecodeError::Unsupported` rather than a
  synthesized default, which would decode SBR/PS streams to quietly wrong output. Raw AAC
  only — ADTS input must be de-headered first (`adts-core`).

- `mediaway_container::MuxOpen` and `mediaway_container::ContainerError`. `MuxOpen` names
  the track-registration phase every muxer already had (`add_track` → `begin`), plus a
  `FIRST_TRACK_ID` const for the container's own track-numbering floor — Matroska reserves
  `TrackNumber` 0, ISOBMFF does not.

- `EncodeSession::open_in` / `open_in_with_audio` — open a session against a
  caller-supplied muxer. This is what makes non-MP4 containers reachable through the facade
  (`EncodeSession::open_in(webm::Muxer::new(), encoder)`), and it is also the only way to
  reach muxer options the facade does not mirror — `mp4::Muxer::with_fragment_batch` was
  previously unreachable, pinning fragment cadence at the default 30
  (`crates/mediaway/adr/0007-encode-session-generic-muxer.md`).

- `Mux::set_track_extra_data`, defaulted to a no-op. Moves ADR-0005's late-known
  extra-data backfill onto the trait. **Containers that commit their track header at
  `begin()` cannot honour it and drop it silently** — `webm::Muxer` is that case, so a
  late-config encoder backend (e.g. `VideoToolbox`) paired with WebM produces a file with
  no codec configuration record. Pair those backends with `mp4::Muxer`.

- `mediaway_encoder::windows::auto::support_at` and `mediaway::platform::encoder_support_at`
  — probe encoder availability at a caller-supplied resolution. Encoder support is
  resolution-dependent; the resolution-free `support`/`encoder_support` forms remain and now
  delegate at a documented default
  (`crates/mediaway-encoder/adr/0005-resolution-aware-capability-probe.md`).

- `EncodeSession::poll_bytes` and `EncodeSession::finish_into` — drain fMP4 bytes
  incrementally during a session instead of holding the whole recording in RAM until
  `finish()`. A long capture's memory is now bounded by poll cadence rather than by
  duration (`crates/mediaway/adr/0006-encode-session-streaming-bytes.md`).

### Changed

- `EncodeSession` is now generic over its muxer:
  `EncodeSession<E: VideoEncoder, M: MuxOpen = mp4::Muxer<mp4::Open>>`. `open` and
  `open_with_audio` keep their exact signatures and still produce fragmented MP4, so no
  existing call site changed.

- `EncodeSession::finish` is now a convenience wrapper over `finish_into`. Its signature
  and its behaviour for a session that was never polled are unchanged; a session drained
  with `poll_bytes` receives only the unpolled tail, as documented on both methods.

### Fixed

- **Opus in MP4 was written as AAC.** `iso-bmff` shared AAC's `mp4a`/`esds` branch for
  `Codec::Opus`, so an Opus track got an `esds` declaring `objectTypeIndication` 0x40
  (MPEG-4 AAC) plus a hardcoded AAC `AudioSpecificConfig`. The file muxed without error and
  failed at playback. Opus now writes a real `Opus` sample entry with a `dOps`
  (`OpusSpecificBox`), and demux recognizes it, yielding an RFC 7845 `OpusHead` in
  `extra_data` so the configuration survives an MP4 → WebM remux
  (`crates/iso-bmff/adr/0005-opus-sample-entry.md`). Files written with an Opus track by an
  earlier version are invalid and must be remuxed.


- `cargo nextest run --workspace` no longer takes over the developer's desktop. Two
  screen-capture smoke tests nudged the mouse cursor and a third opened a visible window;
  all three are now `#[ignore]`d and opt-in via `--run-ignored all`. The convention now
  lists the Win32 calls that trigger the rule, because the first pass was written from the
  reported symptom ("moves the mouse") and missed the window-opening case.

- **DX11 Zero-Copy encode now works on NVIDIA hardware.** Three async-MFT sequencing bugs
  made `VideoInputPreference::ZeroCopyGpu` fail on every machine that selected an async
  encoder MFT — which is every NVIDIA machine, since Intel QuickSync's MFTs are sync and
  NVIDIA's are not: the async unlock came after `MFT_MESSAGE_SET_D3D_MANAGER` (so `open`
  failed with `MF_E_TRANSFORM_ASYNC_LOCKED`), `METransformHaveOutput` was received and
  discarded (so the first `push_frame` failed), and the input wait never drained output (so
  the second `push_frame` deadlocked). Verified on an RTX 4090 at 1920×1080 for H.264 and
  HEVC, from NV12 **and** BGRA input
  (`crates/mediaway-encoder/adr/windows/0012-async-mft-zero-copy-sequencing.md`).
  This was previously recorded in the wiki as an unexplained "environment gap"; that
  diagnosis is retired.

  **That verification is encoder-level.** It asserts the MFT produces packets, and says
  nothing about whether those packets mux into a playable file. For HEVC they did not, until
  the container fix under **Fixed** above — wording it as a bare "verified" is part of how
  that stayed invisible.

- Encoder capability probe no longer reports `NoDevice` for backends that work. It opened
  every probe session at 64×64, below NVENC's minimum dimensions, so `encoder_support`
  reported no NVIDIA encoder on an RTX 4090 that encodes AV1 and H.264 fine. The default
  probe size is now one measured to clear every backend's minimum.

### Removed

### Deprecated

### Breaking

- `ProcessTreeScope::ProcessOnly` is renamed `ProcessTreeScope::ExcludeProcessTree` (and
  `WasapiProcessTreeScope::ProcessOnly` likewise), because it selected the "capture
  everything *except* this process tree" Windows mode while claiming the opposite. Callers
  that meant "the target process" want `IncludeChildren`; Windows offers no narrower mode.
  The C ABI's `mediaway_desktop_audio_capture_config_t.include_child_processes` is renamed
  `include_target_process_tree` for the same reason — same polarity, same struct layout, so
  only source references change. The C#/Python bindings' parameter names follow.

- `CaptureError` gained a `BackendCode { code: i32 }` variant carrying the platform's own
  status code (`HRESULT`, …). The enum is `#[non_exhaustive]`, so only exhaustive matches
  written inside this workspace are affected; `CaptureError::Backend` still exists for
  backends that have no such code.

- `PipelineError::Mux` now wraps `mediaway_container::ContainerError` instead of
  `mediaway_container::mp4::Error`. `EncodeSession` is generic over its container, so the
  variant could not keep naming one container's error type without making `PipelineError`
  itself generic. Matching on a specific container's error still works, one level deeper:
  `PipelineError::Mux(ContainerError::Mp4(mp4::Error::InvalidTrack))`.

