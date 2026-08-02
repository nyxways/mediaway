# mediaway-avcli — roadmap

## Stage 0 — Scaffold

- [x] Crate + docs/adr layout
- [x] Arg parser (sans-io parse → structs; I/O in adapters)
- [x] Wire mux pipeline for a minimal subset (no encoder wired yet — mux only)

## Stage 1 — MVP subset

- [x] Document supported vs unsupported flags (honest matrix) — see `adr/0001-avcli-flag-subset.md`
- [x] Map common mux flows onto Mediaway crates (`-i`/`--synthetic` → `mediaway-container::mp4::Muxer`)
- [x] Exit codes and diagnostics comparable to user expectations (0/1/2)

## Later

- [ ] Real encoder wiring (`mediaway-encoder`) once available — today this tool only muxes pre-encoded bytes
- [ ] Multi-access-unit `-i` input (needs a public AU splitter; `iso-bmff`'s Annex-B NAL iterator is private today)
- [ ] Broader flag coverage only when Mediaway backends exist
- [ ] Never require system FFmpeg at runtime
