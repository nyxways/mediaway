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
- `mediaway-decoder`: `AudioDecoder` trait, mirroring `VideoDecoder`
  (`adr/0003-audio-decoder-trait.md`). Implemented for the WMF Opus decoder
  (`windows::WmfOpusDecoder`) and `mediaway-sw`'s software Opus decoder
  (`SwOpusAudioDecoder`, wrapping `mediaway_sw::opus::OpusDecoder`).

### Changed

- `mediaway-ffi`: shared C header value types (`mediaway_rational_t`, pixel/sample
  formats, GPU device/buffer handles) moved into a new `include/mediaway/common.h`,
  included by `container.h`/`device.h`/`pipeline.h` instead of each redefining them.
- `mediaway-ffi`: adopted `cbindgen` tooling (`cbindgen.toml`,
  `tools/scripts/cbindgen-headers.ts generate|verify`) — produces a clean-compiling
  generated header for the whole crate; the shipped `include/mediaway/*.h` headers
  stay hand-written pending a separate per-header migration.

### Fixed

- `mediaway-decoder/tests/cpu_roundtrip.rs` (moved from a `tests/windows/` subpath
  `cargo test` never discovered) now actually compiles and runs; found a real,
  unresolved bug in `WindowsVideoDecoder`'s CPU H.264 decode path — left
  `#[ignore]`d with the finding documented, not silently passing.
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

### Removed

### Deprecated

### Breaking
