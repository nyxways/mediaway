# mediaway-avprobe — roadmap

## Stage 0 — Scaffold

- [x] Crate + docs/adr layout
- [x] Arg parser (sans-io parse → structs)
- [x] Text / JSON reporters over demux metadata

## Stage 1 — MVP subset

- [x] Common probe flags used by scripts; explicit unsupported set (see `adr/0001-probe-flag-subset.md`)
- [x] Stream/format summaries from Mediaway demux
- [x] Never require system ffprobe at runtime

## Later

- [ ] Broader coverage as demux backends land
