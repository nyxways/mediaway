# mediaway-container ADRs

Workspace-level facade traits and adapter decisions live here when needed.

| ID | Title |
|----|-------|
| [0001](0001-webm-ebml-demux.md) | WebM demux via a new unprefixed `ebml-webm` core |
| [0002](0002-audio-and-general-container-facades.md) | Facade modules for `riff-wave`/`adts`/`mpeg-audio`/`ogg`/`flv`/`mpeg-ts` |

Format-specific sans-io cores use **unprefixed** crates with their own `adr/` (e.g. [`iso-bmff`](../../iso-bmff/adr/), [`iso-cenc`](../../iso-cenc/adr/)). Thin Mediaway adapters live in this facade — not separate `mediaway-container-<format>` crates (ADR-0012).

Workspace packaging: [`docs/adr/0003-crate-packaging.md`](../../../docs/adr/0003-crate-packaging.md).  
Naming (unprefixed cores): [`docs/adr/0012-unprefixed-reusable-cores.md`](../../../docs/adr/0012-unprefixed-reusable-cores.md).
