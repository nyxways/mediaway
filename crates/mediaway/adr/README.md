# mediaway — ADRs

The crate's existence and initial shape (`EncodeSession`, platform dispatch
migration, `PipelineError`) are covered by the workspace-level
[ADR-0014](../../../docs/adr/0014-pipeline-convenience-crate.md), since it's a
cross-cutting decision (composes multiple facades). Crate-local decisions since
then:

| ID | Title | Status |
|----|-------|--------|
| [0001](0001-frame-filter-hook.md) | Mid-pipeline frame filter hook on `EncodeSession` | Proposed |
| [0002](0002-platform-dispatch-avoid-box-dyn.md) | Platform dispatch shape (avoid `Box<dyn>`) | Accepted |
| [0003](0003-audio-track-and-apm-integration.md) | Audio track support on `EncodeSession`, with optional `mediaway-audio-apm` (AEC/NS/AGC + VAD) integration | Accepted |

Template: [`template.md`](template.md)

Crate-local only, for future decisions scoped to this crate. Workspace ADRs:
[`docs/adr/`](../../../docs/adr/).
