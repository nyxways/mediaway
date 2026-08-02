# iso-cenc — roadmap

Sans-IO ClearKey CENC (unprefixed). Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 1 — `cenc` (AES-128-CTR)

- [x] Crate + ADR-0011 / naming ADR-0012
- [x] Subsample-aware CTR (clear ranges do not advance counter)
- [x] Used by `iso-bmff` demux (`tenc` / `senc`)

### 2 — Pattern / CBC schemes

- [ ] `cens`, `cbc1`, `cbcs` when a concrete product need appears
