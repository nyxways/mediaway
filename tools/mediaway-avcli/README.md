# mediaway-avcli

<p align="center">
  <a href="https://crates.io/crates/mediaway-avcli"><img src="https://img.shields.io/crates/v/mediaway-avcli.svg" alt="crates.io"></a>
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production. Part of the
> [Mediaway](https://github.com/nyxways/mediaway) media stack. **Not affiliated with the
> FFmpeg project** — a Mediaway-native CLI with a familiar argument subset for migration
> convenience.

An AV CLI for muxing: takes pre-encoded access units (or synthetic packets) and writes a
container with Mediaway. Today it muxes only — encoder wiring lands as backends mature.

## Quick start

```bash
# Mux one access unit (read from stdin, written to out.mp4)
mediaway-avcli -i input.h264 out.mp4

# Mediaway-native self-test: mux 30 synthetic H.264 packets
mediaway-avcli --synthetic 30 out.mp4
```

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| Arg parser (sans-io parse → structs) | ✅ | `-i`/`--synthetic`, `-s WxH`, `-y`, output path (`-` = stdout) |
| Mux pipeline (`mediaway-container::mp4`) | ✅ | Mux-only MVP |
| Exit codes and diagnostics (0/1/2) | ✅ | |
| Encoder wiring | 🛠️ | Planned once `mediaway-encoder` paths are stable |

## Docs

- [`docs/roadmap.md`](docs/roadmap.md) — stages, deferred items, design decisions
- [`mediaway-avprobe`](../mediaway-avprobe/) — the companion probe CLI
- Root [README](../../README.md) — workspace overview

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
