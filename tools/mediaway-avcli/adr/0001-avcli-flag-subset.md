# ADR-0001: Supported `mediaway-avcli` flag subset and mux pipeline wiring

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `tools/mediaway-avcli`

## Context

The crate started as a scaffold with two hardcoded toggles (`--count <n>`,
`--stdin`) that always wrote MP4 bytes to stdout: a synthetic multi-packet
demo, or a single access unit read whole from stdin. No real arg parser, no
output file, no encoder wired. The roadmap (Stage 0/1) asks for a real arg
parser, wiring onto the mux pipeline for a minimal subset, and honest
supported/unsupported documentation — without inventing capability
`mediaway-container`/`iso-bmff` do not have (no H.264 encoder crate is wired
into this workspace yet; this tool only **muxes** already-encoded bytes).

## Decision

> Support a small, explicit ffmpeg-shaped flag subset that drives the
> existing mux pipeline; keep the synthetic self-test path but name it
> honestly as Mediaway-native, not an ffmpeg flag.

- `-i <input>` (`-` = stdin): source of one H.264 Annex-B access unit, muxed
  as a single keyframe packet. Generalizes the prior `--stdin` demo to files.
- Positional output path (`-` = stdout), **required**.
- `-s WxH`: video geometry override (default `1920x1080`, the prior hardcoded
  value).
- `-y`: accepted as a no-op — Mediaway never prompts before overwriting, so
  there is nothing for it to suppress; kept only for ffmpeg script compat.
- `--synthetic <n>`: Mediaway-native self-test mode (**not** an ffmpeg flag)
  generating `n` synthetic H.264 packets — this is the prior `--count` demo,
  now flag-named honestly and driven by the same parser. Mutually exclusive
  with `-i`.
- Unknown flags, missing required values, or `-i`+`--synthetic` together are
  usage errors (exit `2`). I/O or mux failures exit `1`.
- No real encoder: this tool does not parse an arbitrary raw elementary
  stream into multiple access units (no AU delimiter is exposed by
  `iso-bmff`'s public API yet) — `-i` mode stays single-packet until that
  capability exists upstream.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Full ffmpeg flag surface (`-c:v`, `-b:v`, `-r`, `-an`, filters, …) | No encoder crate wired; would document flags with no real effect |
| Parse `-i` input into multiple access units via NAL scanning | `iso-bmff`'s Annex-B NAL iterator is a private implementation detail, not a public API to build on; adding a public AU splitter is a larger, separate design decision |
| Drop `--synthetic` entirely | Throws away the only working multi-packet mux demo/self-test path the crate has |

## Consequences

### Positive

- `mediaway-avcli --synthetic <n> out.mp4` and
  `mediaway-avcli -i in.h264 [-s WxH] out.mp4` are real, flag-driven, tested
  paths instead of hardcoded toggles.
- Output now writes to a real file (or stdout via `-`), not stdout-only.

### Negative / Trade-offs

- `-i` mode is single-access-unit only; multi-frame real files need the
  `--synthetic` self-test path or a future AU-splitting capability.
- No actual encoding happens — this remains a mux-only tool until
  `mediaway-encoder` is wired in.

## References

- `tools/mediaway-avcli/docs/roadmap.md`
- `crates/mediaway-container/src/mp4.rs` (`Muxer`)
- `crates/iso-bmff/src/bitstream/avc.rs` (internal Annex-B → AVCC, not a public AU splitter)
