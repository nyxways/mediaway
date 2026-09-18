# Mediaway release notes

<!-- Accumulate development changes under ## Unreleased

### Added

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

- Encoder capability probe no longer reports `NoDevice` for backends that work. It opened
  every probe session at 64×64, below NVENC's minimum dimensions, so `encoder_support`
  reported no NVIDIA encoder on an RTX 4090 that encodes AV1 and H.264 fine. The default
  probe size is now one measured to clear every backend's minimum.

### Removed

### Deprecated

### Breaking

- `PipelineError::Mux` now wraps `mediaway_container::ContainerError` instead of
  `mediaway_container::mp4::Error`. `EncodeSession` is generic over its container, so the
  variant could not keep naming one container's error type without making `PipelineError`
  itself generic. Matching on a specific container's error still works, one level deeper:
  `PipelineError::Mux(ContainerError::Mp4(mp4::Error::InvalidTrack))`.

