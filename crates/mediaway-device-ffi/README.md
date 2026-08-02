# mediaway-device-ffi

C ABI facade over [`mediaway-device`](../mediaway-device/README.md) (Camera video capture; Microphone/Loopback/ProcessLoopback audio capture). Third `*-ffi` crate in the workspace, after [`mediaway-container-ffi`](../mediaway-container-ffi/README.md) and [`mediaway-pipeline-ffi`](../mediaway-pipeline-ffi/README.md).

**Status:** scaffold only — no exported functions, no header, no ABI has shipped yet.

Design rules: [`docs/spec/c-ffi.md`](../../docs/spec/c-ffi.md) (ADR-0004) · packaging: [`docs/spec/crate-packaging.md`](../../docs/spec/crate-packaging.md) (ADR-0003).

See `docs/roadmap.md` for stages and `adr/` for crate-local design decisions.
