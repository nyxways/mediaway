# mediaway-encoder — ADRs

| ID | Title | Status |
|----|-------|--------|
| [0001](0001-encoder-traits.md) | `VideoEncoder` / `AudioEncoder` streaming traits | Accepted |
| [0002](0002-facade-platform-boundary.md) | Facade vs `mediaway-encoder-<platform>` | Accepted |
| [0003](0003-auto-encode.md) | `auto` encode surface | Accepted |
| [0004](0004-backend-preference.md) | Backend preference hierarchy (Auto/Os/Gpu/Sw) | Accepted |
| [0005](0005-resolution-aware-capability-probe.md) | Resolution-aware encoder capability probe (`support_at`) | Accepted |
| [0006](0006-finish-ends-a-stream.md) | `finish(self)` ends a stream; dropping discards what is in flight | Accepted |

Template: [`template.md`](template.md)

Crate-local only. Workspace ADRs: [`docs/adr/`](../../../docs/adr/).
