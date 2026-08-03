# mediaway-pipeline-ffi

C ABI facade over [`mediaway-pipeline`](../mediaway-pipeline/README.md) (auto video encode -> fragmented MP4). Second `*-ffi` crate in the workspace, after [`mediaway-container-ffi`](../mediaway-container-ffi/README.md).

**Status:** scaffold only — no exported functions, no header, no ABI has shipped yet.

Design rules: [`docs/spec/c-ffi.md`](../../docs/spec/c-ffi.md) (ADR-0004) · packaging: [`docs/spec/crate-packaging.md`](../../docs/spec/crate-packaging.md) (ADR-0003).

See `docs/roadmap.md` for stages and `adr/` for crate-local design decisions.
