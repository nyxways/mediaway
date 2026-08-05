# C-FFI (C ABI) surface

Canonical decision: [`docs/adr/0004-c-ffi.md`](../adr/0004-c-ffi.md) — **packaging superseded
2026-08-03** by [`docs/adr/0021-workspace-consolidation.md`](../adr/0021-workspace-consolidation.md).

Mediaway exposes a **C ABI** so non-Rust languages can call into the stack. The Rust API stays primary; FFI is a dedicated edge.

## Packaging (single crate, feature-gated modules)

> ADR-0021 merged the four `mediaway-*-ffi` crates (`mediaway-common-ffi`,
> `mediaway-container-ffi`, `mediaway-device-ffi`, and the standalone
> `mediaway-ffi`) into **one** `mediaway-ffi` crate. The selectivity goal ADR-0004
> set out is unchanged — it now lives in **Cargo features on that single crate**
> instead of separate crate boundaries. Do not add a second `-ffi` crate for a new
> capability; add a module + feature(s) to `mediaway-ffi` instead.

`mediaway-ffi` ships one `cdylib`/`staticlib`, one header set
(`include/mediaway/{common,container,device,pipeline}.h`), and four modules:

| Module | Cargo feature(s) | Rust dep(s) gated by that feature |
|--------|-------------------|-------------------------------------|
| `common` | always compiled | `mediaway-common` |
| `container` (mux/demux) | `mux`, `demux` | `mediaway-container` — `mux`/`demux` propagate to the matching `mediaway-container/{mux,demux,audio,video}` sub-features (feature unification), not a hardcoded full build |
| `device` (camera/desktop/audio capture/hotplug) | `camera`, `desktop`, `audio`, `hotplug` | `mediaway-device` |
| `pipeline` (auto encode/decode, Opus audio decode) | `pipeline` | `mediaway`, `mediaway-encoder`, `mediaway-decoder`, `mediaway-sw` (all `optional = true`) |

```text
[host language]
    → mediaway_ffi.dll/.so/.dylib built with features=["mux","demux"]                 (slim: container-only)
    → or features=["mux","demux","camera","desktop","audio","hotplug","pipeline"]     (full — current default)
        → Rust facades / sans-io / platform crates
[Rust apps] → facades / platform crates directly (preferred)
```

### Feature rules

- Each capability's features gate **both** the FFI wrapper module (`#[cfg(feature = "…")]` on the `pub mod` / `pub use`) **and** the underlying Rust dependency (`optional = true` + feature propagation) — enabling one capability must not pull another's heavy deps (e.g. `mux`+`demux` alone must not link `mediaway-encoder`/`mediaway-decoder`/`mediaway-sw`).
- `default` currently enables every feature (`mux`, `demux`, `camera`, `desktop`, `audio`, `hotplug`, `pipeline`) — the pre-ADR-0021 full build, kept as the default so existing consumers see no behavior change.
- A symbol that spans two capabilities (e.g. the capture→encode bridge, which needs both `pipeline` and `camera`/`desktop`) is gated on the conjunction of both features, not just one.
- CI builds at least one **slim** feature set (e.g. mux-only) and one fuller set. *(Not wired into CI yet — the crate-level feature gating this describes is a prerequisite, not yet a CI job.)*

### About `mediaway-common`

Depending on `mediaway-common` is **expected and fine** — it is shared types, kept small, and always compiled. Avoid linking a capability's deps at a feature level fuller than what's actually enabled; that is what per-module features prevent.

### Artifacts

| Item | Value |
|------|--------|
| Crate crate-types | `rlib` + `cdylib` + `staticlib` |
| Headers | `include/mediaway/{common,container,device,pipeline}.h` |
| Depends | Downward only onto Rust capability crates — **never** the reverse |

## Planned language support

**Status:** aspirational list for `0.x` planning. Nothing here is shipped or ABI-stable yet ([`status.md`](status.md)).

### Tier A — first-class Rust

| Language | Interop | Notes |
|----------|---------|--------|
| **Rust** | Native crates | Primary API; always preferred when the host is Rust |

### Tier B — via C ABI (`*-ffi` / optional umbrella)

Consumers link the headers + libraries for **only the capabilities they need**. In-tree idiomatic wrappers may come **after** the C ABI exists.

| Language | Planned path | Priority rationale |
|----------|--------------|-------------------|
| **C** | Direct | ABI contract language |
| **C++** | Direct / thin wrapper | Native apps, engines, tools |
| **C#** | P/Invoke (optional later: NuGet helper) | Windows desktop, tooling, engine plugins |
| **Python** | `ctypes` / `cffi` (optional later: wheel) | Scripts, tests, pipelines |
| **Zig** | `@cImport` | Systems hosts, easy C interop |
| **Go** | `cgo` | Services / tooling that stay off the hot GPU path |
| **Swift** | C bridging header | Aligns with Apple platform track |
| **Kotlin / Java** | JNI over the C ABI | Aligns with Android platform track |
| **JavaScript / TypeScript (Node.js)** | Native addon or FFI over the C ABI (e.g. N-API / `napi-rs` / koffi) | Server / tooling — link slim `-ffi` sets |

Order of **official in-tree wrappers** (if any): prefer **C#**, **Python**, and **Node (JS/TS)** after C/C++ consumers work — exact sequence is a later ADR.

### Tier C — Web browser (not C-FFI)

| Language | Interop | Notes |
|----------|---------|--------|
| **JavaScript / TypeScript (browser)** | WASM (`wasm-bindgen` / WebCodecs / WebGPU) | Web platform track; **does not** use `*-ffi` |

So JS/TS has **two** planned hosts: **Node → C ABI**, **browser → WASM**. Do not collapse them into one interop story.

### Explicitly out of scope (for now)

- Guaranteeing every Tier B language on day one of the first `-ffi` crate
- A mandatory all-capabilities `cdylib` with no feature opt-out
- Maintaining bindings for languages not listed above without a new ADR
- Replacing Rust Zero-Copy types (`GpuBufferHandle`, shared CPU buffers) with lowest-common-denominator forced-copy FFI

## Design rules

1. Map existing Rust surfaces; do not invent C-only capabilities.
2. Opaque handles + error codes; no panic across FFI.
3. Document ownership / thread safety per function in the header.
4. Preserve Zero-Copy where the Rust path allows (pass GPU/OS tokens **or** shared CPU buffer ownership explicitly).
5. Pre-1.0: ABI may break; export version macros per library/header.
6. **Web browser** uses WASM / Web APIs — not this C ABI. **Node.js JS/TS** uses the C ABI (Tier B).

## When

Landed: `mediaway-ffi` ships the `common`/`container`/`device`/`pipeline` modules today (ADR-0021). No header/ABI has been released yet — pre-1.0, breaking changes expected (see [`status.md`](status.md)). See `crates/mediaway-ffi/docs/{common,container,device,pipeline}/roadmap.md` for module-level status.

## Out of scope (initially)

- Full API parity with Rust
- Shipping Tier B language packages in the first FFI milestone (C ABI first; wrappers later)
