# Encoder crate scaffold

- Facade `mediaway-encoder`: traits + `auto` types (`Config::new`, path/policy)
- **Cargo features** (ADR-0004): `audio`, `video`, `full` (default). Platform modules mirror (`mediaway-encoder::windows`, `mediaway-encoder::web`).
- Windows: H.264 / HEVC / AV1 / VP9 via WMF (`WmfVideoEncoder`); H.264 DX11 ZC README ⚡
- Umbrella (planned): `mediaway-codec`
- Next: GpuCopy BGRA→NV12; promote multi-codec cells with CI proof
