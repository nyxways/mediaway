# mediaway-avprobe

<p align="center">
  <a href="https://crates.io/crates/mediaway-avprobe"><img src="https://img.shields.io/crates/v/mediaway-avprobe.svg" alt="crates.io"></a>
  <a href="https://github.com/nyxways/mediaway"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="License"></a>
</p>

> **Status:** early development (`0.x`). Not recommended for production. Part of the
> [Mediaway](https://github.com/nyxways/mediaway) media stack. **Not affiliated with the
> FFmpeg project** — a Mediaway-native probe with a familiar argument subset for
> migration convenience.

A media metadata probe: demuxes with Mediaway and reports stream/format summaries as
human-readable text or JSON.

## Quick start

```bash
# Default: format + stream summaries
mediaway-avprobe input.mp4

# Explicit flags, JSON output
mediaway-avprobe -show_format -show_streams -of json input.mp4
```

## Status

Status marks: ✅ first-class (tests for claimed scope) · ⚡ Zero-Copy path (no payload copy; implies ✅) · 🆗 best-effort / prototype · 🛠️ planned · ❌ attempted and genuinely blocked · 👻 not exercisable yet (no hardware / device / session available)

| Area | Status | Notes |
| ---- | ------ | ----- |
| Arg parser (sans-io parse → structs) | ✅ | positional input, `-show_format`, `-show_streams`, `-of default\|json` |
| Demux metadata reporters (text + JSON) | ✅ | |
| Never requires system ffprobe | ✅ | |
| Broader flag coverage | 🛠️ | As demux backends land |

## Docs

- [`docs/roadmap.md`](docs/roadmap.md) — stages, deferred items, design decisions
- [`mediaway-avcli`](../mediaway-avcli/) — the companion mux CLI
- Root [README](../../README.md) — workspace overview

## Contributing

Contributions are welcome — open an issue or pull request at
[github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).

## License

MIT OR Apache-2.0.
