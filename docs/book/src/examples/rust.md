# Rust

The primary, always-first-class API. Every capability is reachable from native crates on
crates.io (`mediaway`, `mediaway-encoder`, `mediaway-decoder`, `mediaway-device`,
`mediaway-container`, the `*-core` format crates, …).

## Install

```bash
cargo add mediaway            # umbrella: pipeline + platform dispatch + re-exports
# or a specific capability / freestanding core:
cargo add mediaway-encoder mediaway-container iso-bmff ogg-core
```

Runnable examples live in the workspace [`examples/`](https://github.com/nyxways/mediaway/tree/main/examples)
directory, grouped by capability:

| Capability | Example | Run |
|------------|---------|-----|
| Container | `container/mux_demux_mp4.rs` | `cargo run --example mux_demux_mp4` |
| Encode | `encode/encode_h264.rs` | `cargo run --example encode_h264` |
| Decode | `decode/decode_h264.rs` | `cargo run --example decode_h264` |
| Device | `device/capture_camera.rs` · `capture_microphone.rs` · `capture_screen.rs` · `capture_window.rs` | `cargo run --example capture_screen` |
| Pipeline | `pipeline/encode_to_mp4.rs` · `screen_record.rs` · `trim_and_splice.rs` | `cargo run --example screen_record` |

The guides in this book walk through the same flows with annotated code:

- [Container: Mux + Demux](../guides/container.md)
- [Encode](../guides/encode.md) · [Decode](../guides/decode.md)
- [Device](../guides/device.md)
- [Pipelines: Composing It All](../guides/pipelines.md)

API reference: [Crate Docs](../reference/crate-docs.md) (docs.rs).
