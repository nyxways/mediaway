# GOP / CBR / set_bitrate reach the C ABI + C# (2026-08-07)

- ADR: `crates/mediaway-ffi/adr/pipeline/0001-auto-encode-c-abi.md`'s 2026-08-07
  addendum. ABI bump: `MEDIAWAY_PIPELINE_FFI_ABI_VERSION` 5 → 6.
- Rust source of truth: `mediaway_encoder::VideoEncoderConfig::gop_size`/
  `rate_control` (real on the Vulkan H.264/HEVC encoders,
  [vulkan-h264-gop](../encode/vulkan-h264-gop.md)) and `VideoEncoder::set_bitrate`
  (trait method, default `Err(Unsupported)`).

## What changed

- `mediaway_encoder::auto::AutoVideoEncodeConfig` gained `gop_size`/
  `rate_control` fields, threaded through `to_low_level` (previously hardcoded
  to `1`/`None`).
- `mediaway_auto_video_encode_config_t` mirrors both — `rate_control`'s
  `Option<RateControlConfig>` flattens into `rate_control_enabled: bool` +
  `rate_control_target_bitrate_bps: u32` + `rate_control_vbv_buffer_size_bytes:
  u32` (`0` = driver default), no C union — same idiom this struct's own
  `bitrate_bps` already uses.
- New `mediaway_encode_session_set_bitrate(session, bitrate_bps)` C export,
  backed by a new `mediaway::EncodeSession::set_bitrate` passthrough and a
  `set_bitrate` override on `AutoEncoderHandle`'s `VideoEncoder` impl (it
  previously fell back to the trait's default `Unsupported`, even though the
  boxed inner encoder might support it).
- `Mediaway.Pipeline` C#: `VideoEncodeConfig.GopSize`/`RateControl` (new
  `RateControlConfig` record) and `EncodeSession.SetBitrate`.

## Honest scope limit — read before using

`mediaway::platform::AutoEncoder` (what every FFI/C#/other-binding caller
reaches via `mediaway_auto_encoder_open`) never resolves to the Vulkan
backend — Vulkan Video isn't part of `BackendSelection` yet
([backend-preference](../encode/backend-preference.md)). So:

- `gop_size`/`rate_control`: **silent no-op** through this path today. The
  auto-selected backend (WMF on Windows, VA-API on Linux) ignores both fields
  and keeps producing IDR-only/fixed-QP output, byte-identical to before, with
  no error — the same capability-gated-fallback contract
  `VideoEncoderConfig::gop_size` itself documents, just currently gated at
  100% on every backend this crate can open.
- `set_bitrate`: **fails loudly instead** — `MEDIAWAY_PIPELINE_STATUS_UNSUPPORTED`
  / `MediawayPipelineStatus.Unsupported`, since WMF doesn't override
  `set_bitrate` either. Verified by
  `bindings/csharp/tests/Mediaway.Pipeline.Tests/EncodeToMp4Tests.cs`'s
  `SetBitrate_OnWmfSession_ThrowsUnsupported`.

Wiring Vulkan into the auto-select path (or exposing a Vulkan-specific open
function) so these fields actually take effect from C/C# is a follow-up, not
decided here.

## Tests

- Rust: `crates/mediaway/src/session_tests.rs::set_bitrate_forwards_to_the_underlying_encoder`.
- C#: `EncodeToMp4Tests.GopSizeAndRateControl_DoNotBreakEncode_ButAreNotYetHonoredByWmf`,
  `EncodeToMp4Tests.SetBitrate_OnWmfSession_ThrowsUnsupported` — both hardware-verified
  (real WMF encoder on this machine).
