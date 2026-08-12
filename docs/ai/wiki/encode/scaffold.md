# Encoder crate scaffold

- Facade `mediaway-encoder`: traits + `auto` types (`Config::new`, path/policy)
- **Cargo features** (ADR-0004): `audio`, `video`, `full` (default). Platform modules mirror (`mediaway-encoder::windows`, `mediaway-encoder::web`).
- Windows: H.264 / HEVC / AV1 / VP9 via WMF (`WmfVideoEncoder`); H.264 DX11 ZC README ⚡
- `VideoEncoderConfig::color_range` (`ColorRange::Video`/`Full`, `mediaway-common`, 2026-08-12):
  YUV sample range for `pixel_format`. Only the Apple `VideoToolbox` backend honors it today
  (`kCVPixelFormatType_420YpCbCr8BiPlanar{Video,Full}Range`); Windows/Linux/Android accept the
  field but don't yet branch on it (implicit native default), same capability-gated-fallback
  convention as `gop_size`/`rate_control`.
- Umbrella (planned): `mediaway-codec`
- Next: GpuCopy BGRA→NV12; promote multi-codec cells with CI proof
