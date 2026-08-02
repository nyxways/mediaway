# System CLI test oracle

Canonical: [`docs/adr/0002-system-oracle.md`](../../../adr/0002-system-oracle.md) · [`testing.md`](../../../conventions/testing.md).

- **Encourage** a PATH reference CLI (typically `ffmpeg` / `ffprobe`) for compares, probe, goldens
- **Never** Cargo-dep or link FFmpeg into Mediaway crates (`deny.toml` bans bindings)
- Default `cargo test` must pass **without** an oracle installed (skip / ignore)
- Canonical fixtures from Rust generators — not an external CLI mint
- Product bins: `mediaway-avcli` / `mediaway-avprobe` (not `*ffmpeg*` / `*ffprobe*`)
