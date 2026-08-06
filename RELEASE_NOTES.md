# Mediaway release notes

<!-- Dev changes accumulate under ## Unreleased (AGENTS.md § 10). Finalize
     with `/release-notes <version>`; reset this template with
     `/release-notes reset`. See docs/ai/wiki/meta/release-notes.md. -->

## Unreleased

### Added

- `mediaway-ffi`: video decode C ABI (`mediaway_decode_session_open/push_packet/
  poll_frame/flush/close`, `adr/pipeline/0004-auto-decode-c-abi.md`), wrapping
  `mediaway::platform::AutoDecoder`. CPU output only this pass; blocked from
  hardware verification by a real, pre-existing `WindowsVideoDecoder` bug found
  while adding it (see Fixed).
- `mediaway-ffi`: capture-to-encode bridge
  (`mediaway_encode_session_write_frame_from_{camera,desktop}_capture`,
  `adr/pipeline/0005-capture-encode-bridge-c-abi.md`) — pushes a polled
  Camera/Screen capture frame straight into an encode session with no
  intermediate frame struct and no extra copy (Screen is Zero-Copy end-to-end).
  Hardware-verified with a real USB camera.
- `mediaway-ffi`: Opus audio decode C ABI (`mediaway_audio_decode_session_open/
  push_packet/poll_frame/flush/close`, `adr/pipeline/0006-audio-decode-c-abi.md`),
  wrapping `mediaway_sw::opus::OpusDecoder` directly. Empty-payload packets are
  Opus packet-loss concealment, not an error. `CodecKind::Opus` is also now
  reachable from the existing audio encode C ABI
  (`mediaway_audio_encode_config_opus`) — previously AAC-only despite
  `SwOpusAudioEncoder` already existing at the Rust level. Cross-platform
  (`mediaway-sw` has no OS dependency); round-trip-verified
  (`tests/audio_decode_smoke.rs`).
- `mediaway-encoder`: multi-frame GOP (P-frame prediction) for the Vulkan
  H.264 and HEVC encode backends, plus CBR rate control for H.264
  (`adr/vulkan/0002-vulkan-gop-rate-control.md`). New
  `VideoEncoderConfig::gop_size`/`rate_control` fields (cross-backend; only
  Vulkan H.264/HEVC read them so far, CBR only on H.264). Capability-gated
  (`Capabilities::supports_p_frames`/`supports_cbr`) — falls back to today's
  IDR-only/fixed-QP behavior with no error when the driver can't honor a
  request. Defaults (`gop_size: 1`, `rate_control: None`) keep every
  existing caller's output byte-identical. Hardware-verified on a real RTX
  4090: real IDR/P NAL cadence for both codecs, real CBR bitstream output
  for H.264. `VideoEncoderConfig::gop_size` now also reaches the Vulkan AV1
  backend's internal wiring, but this is **not a working feature** — AV1's
  underlying per-frame encode is already blocked by the same
  driver-maturity bug as its base (IDR-only) path (`adr/0001`'s AV1
  addendum), so the GOP request is capability-gated but its output cannot
  currently be verified.
- `mediaway-encoder`: D3D12 native video-encode backend (still self-contained,
  not wired into the public API) gains real `gop_size > 1` support for H.264
  and HEVC — single forward reference, same capability-gated-fallback
  contract as the Vulkan GOP work above
  (`adr/windows/0007-d3d12-native-video-encode.md`'s 2026-08-06 addendum).
  Hardware-verified on a real RTX 4090: real `IPPIPPI` Annex-B NAL cadence for
  both codecs. AV1 stays all-intra this pass. Also new:
  `VideoEncoderConfig::intra_refresh_period` — row-based intra refresh (unbounded
  GOP + continuous refresh waves instead of periodic IDR) for H.264/HEVC,
  capability-gated on the driver's real `MaxIntraRefreshFrameDuration` cap.
- `mediaway-encoder`: `VideoEncoder::set_bitrate` — live CBR bitrate retargeting mid-session,
  no reopen. Implemented for Vulkan H.264 and the D3D12 native backend's H.264/HEVC (still
  self-contained, not wired into the public API), which also gains real, capability-gated
  CBR rate control (`VideoEncoderConfig::rate_control`). Hardware-verified on a real RTX
  4090: real CBR selected and `set_bitrate` accepted live for both codecs on D3D12 and for
  H.264 on Vulkan.
- `mediaway-decoder`: `AudioDecoder` trait, mirroring `VideoDecoder`
  (`adr/0003-audio-decoder-trait.md`). Implemented for the WMF Opus decoder
  (`windows::WmfOpusDecoder`) and `mediaway-sw`'s software Opus decoder
  (`SwOpusAudioDecoder`, wrapping `mediaway_sw::opus::OpusDecoder`).
- New freestanding sans-io crate `rtp-core`: RTP payloadization for H.264/HEVC
  (RFC 3550/6184/7798) — fixed header build/parse, single-NAL-unit packets, and
  FU-A/FU fragmentation for NAL units larger than a caller-supplied payload
  budget. Closes the workspace's previous no-RTP gap for standard-interop
  low-latency streaming.
- `@mediaway/browser` (ADR-0022): `DecodeSession` — demux-then-decode fMP4 via
  the browser's native WebCodecs `VideoDecoder`/`AudioDecoder`, the decode-side
  mirror of `EncodeSession`. Required `resolveCodec(track)` maps the
  container's generic `Codec` name to the WebCodecs codec string;
  `DecoderUnavailableError` for browsers without a usable decoder. E2E-verified
  H.264 + AAC encode→mux→demux→decode round trips. Also new:
  `examples/device/list-and-watch-devices.ts` (native `enumerateDevices()` +
  `devicechange` hotplug demo).
- `mediaway-ffi`/C#: `gop_size`/`rate_control` (GOP + CBR request) and live
  `set_bitrate` reach the auto-encode C ABI (`mediaway_auto_video_encode_config_t`
  gains 4 fields, ABI v5 → v6; new `mediaway_encode_session_set_bitrate`) and the
  `Mediaway.Pipeline` C# package (`VideoEncodeConfig.GopSize`/`RateControl`,
  `EncodeSession.SetBitrate`) — `adr/pipeline/0001-auto-encode-c-abi.md`'s
  2026-08-07 addendum. Honestly scoped: `mediaway::platform::AutoEncoder` (what
  every FFI/binding caller reaches) never resolves to the Vulkan backend that
  actually reads `gop_size`/`rate_control` yet, so those two are a documented,
  forward-compatible no-op through this path for now (same IDR-only/fixed-QP
  output, no error); `set_bitrate` fails loudly instead
  (`MEDIAWAY_PIPELINE_STATUS_UNSUPPORTED`) since the WMF backend it currently
  reaches doesn't implement it either.

### Changed

- `mediaway-ffi`: shared C header value types (`mediaway_rational_t`, pixel/sample
  formats, GPU device/buffer handles) moved into a new `include/mediaway/common.h`,
  included by `container.h`/`device.h`/`pipeline.h` instead of each redefining them.
- `mediaway-ffi`: adopted `cbindgen` tooling (`cbindgen.toml`,
  `tools/scripts/cbindgen-headers.ts generate|verify`) — produces a clean-compiling
  generated header for the whole crate; the shipped `include/mediaway/*.h` headers
  stay hand-written pending a separate per-header migration.

### Fixed

- `mediaway-encoder`: D3D12 native AV1 backend (still self-contained, not wired into the
  public API) — fixed a wrong-feature-query bug that made this driver's AV1 support look
  entirely absent, plus a real `ReferenceFramesReconPictureDescriptors` DPB-index bug, an
  undersized resolved-metadata buffer, and (later) a subregion-metadata extraction gap that
  both blocked/risked corrupting `EncodeFrame` output. `EncodeFrame` now succeeds with a
  structurally-valid bitstream, but real hardware output still fails `libdav1d` decode at a
  100% error rate — confirmed by direct `ffmpeg`/`libdav1d` testing, root cause still open.
  See ADR-0007's 2026-08-07 addenda for the full account.
- `mediaway-decoder` (Windows): D3D12 native H.264 decode (still self-contained, not wired
  into the public API) — `DXVA_Slice_H264_Long::BitOffsetToSliceData` was computed backwards
  (translated into a raw-buffer-relative offset) against the official DXVA H.264 decoding
  spec, which defines it as the de-emulated-RBSP-relative offset the accelerator itself
  translates internally. Fixed; real hardware `DXGI_ERROR_DEVICE_HUNG` TDR still reproduces
  (8th occurrence) — this was a real, spec-confirmed bug independent of the hang's actual
  cause. See ADR-0002's 2026-08-07 addendum.
- `mediaway-decoder::vulkan`'s HEVC GPU decode no longer produces an all-zero
  picture: `HevcPps` was missing `pps_loop_filter_across_slices_enabled_flag`,
  which gates a real conditional bit in every slice header, desyncing the
  driver's CABAC parser one bit before CTU data; also fixed a
  `general_level_idc` raw-byte-vs-enum-ordinal encoding bug and several
  hardcoded `profile_tier_level` constraint flags. `tests/vulkan/hardware_h264_decode.rs`
  and `tests/vulkan/hardware_hevc_decode.rs` (moved under `tests/vulkan/` by
  an earlier crate merge but never wired into `Cargo.toml`, so `cargo test`
  silently never ran either) are now real `[[test]]` targets again; HEVC's
  hardware test hard-asserts on decoded pixel values instead of soft-skipping.
- `mediaway-decoder` (Windows): `WindowsVideoDecoder`'s CPU H.264 path (`CpuFramesOk`)
  silently decoded **zero frames** for streams fed straight from a WMF encoder (Annex-B
  packets + avcC `extra_data` were treated as AVCC-framed and corrupted). `packet_to_sample`
  now probes each payload for an Annex-B start code and passes Annex-B packets through
  unchanged; demuxed AVCC streams still convert. `tests/cpu_roundtrip.rs` un-`#[ignore]`d.
- `mediaway-ffi`: `mediaway_decode_session_close` teardown crashed the process in
  `tests/decode_smoke.rs` because the test called `mediaway_encode_session_close` after
  `mediaway_encode_session_finish` had already consumed the session (double-free) — stray
  call removed, test un-`#[ignore]`d and passing end-to-end.

### Removed

### Deprecated

### Breaking
