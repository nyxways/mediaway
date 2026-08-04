# ebml-webm ADRs

| ID | Title |
|----|-------|
| [0001](0001-ebml-vint-webm-schema-v1.md) | EBML VINT + WebM demux schema subset (v1) |
| [0002](0002-full-matroska-profile.md) | Lacing, `BlockGroup`, `Audio` fields, `Cues`/`SeekHead` |
| [0003](0003-webm-mux.md) | WebM mux (`Segment`/`Tracks`/`Cluster`/`SimpleBlock` writer) |
| [0004](0004-cluster-lookahead-and-mux-lacing.md) | Indefinite-`Cluster` sibling-ID lookahead + mux lacing |

Workspace packaging: [`docs/adr/0003-crate-packaging.md`](../../../docs/adr/0003-crate-packaging.md).
Naming: [`docs/adr/0012-unprefixed-reusable-cores.md`](../../../docs/adr/0012-unprefixed-reusable-cores.md).
Facade wiring decision: [`mediaway-container/adr/0001`](../../mediaway-container/adr/0001-webm-ebml-demux.md).
