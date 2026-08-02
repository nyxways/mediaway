# CLI tools over the container facade

`tools/mediaway-avprobe` and `tools/mediaway-avcli` are thin, flag-driven
wrappers over `mediaway-container::mp4` (`iso-bmff`, MP4 only today). Both
follow the same shape: hand-rolled arg parser (no `clap`; flag surface is
small — see `docs/conventions/deps-policy.md`) → typed args struct → pipeline
function → exit code `0`/`1`/`2` (`2` = usage error, `1` = runtime error).

## `mediaway-avprobe`

Read the file, demux via `mp4::Demuxer`, build a `ProbeReport` (format +
per-stream summaries), render as text (`-of default`) or hand-rolled JSON
(`-of json`). Flags: `-i`/positional input, `-show_format`, `-show_streams`,
`-of`/`-print_format default|json`.

- Duration is **derived**, not read from a box: `max(pts+duration) - min(pts)`
  across demuxed packets per stream, via `time_base`. There is no
  movie/track-duration getter in the public demux API yet.
- Container `major_brand` comes from the `ftyp` box via the existing
  `mp4_parser::parse_box_tree` helper (raw bytes, not a new box parser).
- Full flag rationale: `tools/mediaway-avprobe/adr/0001-probe-flag-subset.md`.

## `mediaway-avcli`

Mux-only (no encoder crate wired yet). Two modes: `--synthetic <n>`
(Mediaway self-test — canned SPS/PPS/IDR + slice NALs, `n` packets) or
`-i <input>` (`-` = stdin; whole input muxed as **one** keyframe packet —
`iso-bmff`'s Annex-B NAL iterator is a private implementation detail, not a
public access-unit splitter, so multi-frame real elementary streams aren't
split yet). `-s WxH` overrides geometry (default `1920x1080`). `-y` is
accepted as a no-op (Mediaway never prompts before overwriting).

- Full flag rationale: `tools/mediaway-avcli/adr/0001-avcli-flag-subset.md`.

## Testing pattern

Both crates are bin-only (no `src/lib.rs`): unit tests for arg parsing and
(for avprobe) report rendering live as sibling `*_tests.rs` inside the bin
crate itself (normal for `cargo test` on a binary target). Integration tests
spawn the **built binary** via `env!("CARGO_BIN_EXE_<name>")` and either
demux the produced MP4 (avcli) or assert on stdout/exit code (avprobe) — no
`assert_cmd`-style dependency needed. avprobe's fixture MP4 is generated via
`mediaway_test_media::ensure` (BLAKE3-checked, gitignored cache), never
committed.
