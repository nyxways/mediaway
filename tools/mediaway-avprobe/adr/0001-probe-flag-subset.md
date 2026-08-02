# ADR-0001: Supported `mediaway-avprobe` flag subset and report shape

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `tools/mediaway-avprobe`

## Context

The crate started as a scaffold that dumped a raw ISOBMFF box tree for a
hardcoded file path, with no flags. The roadmap (Stage 0/1) asks for a real
arg parser, text/JSON reporters over Mediaway demux metadata, and an explicit
unsupported-flag set — without inventing container/codec support that
`mediaway-container` (MP4 only, via `iso-bmff`) does not yet have.

## Decision

> Support a small, explicit ffprobe-compatible flag subset; reject everything
> else as a usage error instead of silently ignoring it.

- Flags: positional input path, `-i <path>` (alias), `-show_format`,
  `-show_streams`, `-of`/`-print_format default|json`.
- Bare `mediaway-avprobe <file>` (no `-show_*`) shows **both** sections —
  differs from real ffprobe (which prints nothing without `-show_*`), kept
  for a useful default when exploring a file interactively.
- Any other flag → `Usage` error, exit code `2`. Missing/unreadable input or
  a container with zero discoverable streams → exit code `1`.
- Report fields come only from what `mediaway-container` already exposes:
  `StreamInfo` (codec, geometry, time base) plus `ftyp` major brand read via
  the existing `mp4_parser::parse_box_tree` helper.
- **Duration is derived, not read from a box**: there is no movie/track
  duration getter in the public demux API yet, so format/stream duration is
  `(max(pts + duration) - min(pts))` across demuxed packets, converted via
  the stream's `time_base`. `None` when a stream has no packets.
- JSON is hand-rolled (`streams`/`format` keys, ffprobe-shaped) — `serde` is
  not yet a workspace dependency and the field surface is small.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Full ffprobe flag surface (`-show_packets`, `-show_frames`, `-select_streams`, output formats `compact`/`csv`/`flat`/`ini`/`xml`, …) | Far beyond current demux capability; roadmap asks for the common subset first |
| Add `serde`/`serde_json` for JSON output | Not justified yet for ~2 small structs; revisit if the report shape grows |
| Read `mvhd`/`mdhd` duration boxes directly | Not exposed by `mediaway-container`; would duplicate box parsing outside the facade instead of extending it |

## Consequences

### Positive

- Scripts using the common `-show_format -show_streams -of json <file>` shape work.
- Unknown flags fail loudly (exit 2) instead of being silently dropped.
- No panics on bad input (missing file, non-MP4 bytes, zero streams).

### Negative / Trade-offs

- Duration is an approximation from packet timestamps, not authoritative
  container metadata; revisit once a real duration getter exists upstream.
- Output formats other than `default`/`json` are usage errors, not silently
  downgraded.

## References

- `tools/mediaway-avprobe/docs/roadmap.md`
- `crates/mediaway-container/src/mp4.rs`, `mp4_parser.rs`
- `crates/mediaway-common/src/lib.rs` (`StreamInfo`, `Rational`)
