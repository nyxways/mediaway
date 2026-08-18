# Workspace ADR index

**Workspace-wide decisions only** (tooling, monorepo, shared policy).

Crate-specific ADRs live next to code: `crates/<name>/adr/`, `tools/<name>/adr/`.  
See [`docs/conventions/docs-layout.md`](../conventions/docs-layout.md).

| ID | Title | Status |
|----|-------|--------|
| [0001](0001-workspace-bootstrap.md) | Workspace bootstrap | Accepted |
| [0002](0002-system-oracle.md) | System CLI as optional test/dev oracle only | Accepted |
| [0003](0003-crate-packaging.md) | Sans-IO / per-OS backend / facade crate split | Accepted |
| [0004](0004-c-ffi.md) | C-FFI surface for non-Rust languages | Accepted |
| [0005](0005-gpu-interop.md) | GPU framework interop (wgpu and analogs) | Accepted |
| [0006](0006-caveats-and-clarity.md) | Perf/compat caveats honesty + code as primary docs | Accepted |
| [0007](0007-async-and-streaming.md) | Async support + streaming-first APIs | Accepted |
| [0008](0008-no-narrow-vertical-mandate.md) | Drop narrow-vertical / ship-narrow-before-breadth mandate | Accepted |
| [0009](0009-zero-cost-abstractions.md) | ZCA first · minimize `Box` · SmallVec when bounded | Accepted |
| [0010](0010-thiserror-errors.md) | Library errors via `thiserror` | Accepted |
| [0011](0011-clearkey-cenc.md) | ClearKey CENC in-product · no DRM CDM | Accepted |
| [0012](0012-unprefixed-reusable-cores.md) | Crate naming v1 — unprefixed cores vs `mediaway-*` | Accepted |
| [0013](0013-native-handle-and-gpu-device.md) | Typed native handles (`NativeHandle`, `GpuDeviceHandle`) + `StreamInfo` geometry split | Accepted |
| [0014](0014-pipeline-convenience-crate.md) | `mediaway-pipeline` — the convenience layer from api-layers.md | Accepted |
| [0015](0015-common-ffi-unification.md) | `mediaway-common-ffi` — partial unification (value-type mirrors + buffer helper only) | Proposed |
| [0016](0016-cbindgen-ffi-headers.md) | Adopt `cbindgen` for `mediaway-*-ffi` C header generation | Proposed |
| [0017](0017-csharp-binding-package-layout.md) | C# binding package layout, safety design, and build sequence | Accepted |
| [0018](0018-csharp-netstandard20-unity.md) | C# binding Unity support — netstandard2.0 dual-target + separate UPM integration package | Accepted |
| [0019](0019-csharp-device-package-split-and-hotplug-callback.md) | C# `Mediaway.Device.*` package split (mirrors Rust FFI ADR-0004) + real native hotplug callback | Accepted |
| [0020](0020-browser-wasm-npm-package.md) | `@mediaway/browser` WASM + WebCodecs package — container in WASM, codecs from the host, capture from native Web APIs | Accepted |
| [0021](0021-workspace-consolidation.md) | Merge the `mediaway-*` family — platform backends as `#[cfg]`-gated modules, single `mediaway-ffi`, umbrella `mediaway` (amends ADR-0003/0004) | Accepted |
| [0022](0022-browser-decode-session-and-device-dx.md) | Browser decode session (video + audio) + device DX parity examples (amends ADR-0020) | Accepted |
| [0023](0023-msrv-bump-1-96.md) | Bump workspace MSRV from 1.85 to 1.96 (unblocks AMD AMF `shiguredo_amf`) | Accepted |

Template: [`template.md`](template.md)
