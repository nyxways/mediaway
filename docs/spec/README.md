# Spec

Technical design single source of truth. **English only.**

| Doc | Status |
|-----|--------|
| [vision.md](vision.md) | Accepted — product pillars (Zero-Copy = GPU **or** shared CPU) |
| [status.md](status.md) | Accepted — early development; not for production |
| [maturity-bar.md](maturity-bar.md) | Accepted — what a greenfield stack must earn (correctness, stability, perf) |
| [sans-io.md](sans-io.md) | Accepted — maximize sans-io for mux/demux/bitstream/config |
| [api-layers.md](api-layers.md) | Accepted — low-level APIs first-class and usable |
| [crate-packaging.md](crate-packaging.md) | Accepted — sans-io / per-OS / facade crates |
| [c-ffi.md](c-ffi.md) | Accepted — per-capability `*-ffi` + optional feature umbrella |
| [gpu-interop.md](gpu-interop.md) | Accepted — wgpu / WebGPU / Dawn-style GPU Zero-Copy adapters |
| [caveats-and-clarity.md](caveats-and-clarity.md) | Accepted — document costly paths; code must carry the contract |
| [async-and-streaming.md](async-and-streaming.md) | Accepted — streaming-first; async without mandatory runtime in cores |
| [zero-cost-abstractions.md](zero-cost-abstractions.md) | Accepted — ZCA design; minimize `Box`; SmallVec when bounded |
| [iso_14496_12_isobmff.md](iso_14496_12_isobmff.md) | Mediaway ISOBMFF notes + **URL** to ISO (no full text) |
| [iso_23001_7_cenc.md](iso_23001_7_cenc.md) | ClearKey CENC notes + **URL** to ISO/IEC 23001-7 |
| [overview.md](overview.md) | Draft — pipeline and MVP order |

README **⚡** marks: [`../ai/wiki/zero-copy/marks.md`](../ai/wiki/zero-copy/marks.md).


Write new/changed design here first (high level); crate `docs/` / `adr/` hold detail.
