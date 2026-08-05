# mediaway-ffi `common` module ADRs

The original Rust-side unification (former `mediaway-common-ffi` crate) was decided by a
workspace-wide ADR, not a crate-local one — see
[`docs/adr/0015-common-ffi-unification.md`](../../../docs/adr/0015-common-ffi-unification.md),
subsumed by [ADR-0021](../../../docs/adr/0021-workspace-consolidation.md)'s crate merge.

[`0001-shared-header-consolidation.md`](0001-shared-header-consolidation.md) is a
crate-local follow-up: the C header text (`include/mediaway/common.h`) side of that same
unification, which the crate merge alone had not yet delivered.

Workspace C-FFI policy: [`docs/adr/0004-c-ffi.md`](../../../docs/adr/0004-c-ffi.md).
Workspace packaging: [`docs/adr/0003-crate-packaging.md`](../../../docs/adr/0003-crate-packaging.md).
