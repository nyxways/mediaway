# C-FFI (C ABI) surface

Canonical decision: [`docs/adr/0004-c-ffi.md`](../adr/0004-c-ffi.md).

Mediaway will expose a **C ABI** so non-Rust languages can call into the stack. The Rust API stays primary; FFI is a dedicated edge.

## Packaging (split + optional umbrella features)

Do **not** ship one fat FFI that always links every capability.

| Kind | Pattern | Role |
|------|---------|------|
| Per-capability FFI | `mediaway-<capability>-ffi` | Primary unit — e.g. `mediaway-container-ffi` |
| Shared C types (optional) | `mediaway-common-ffi` | Value-type mirrors (`Rational`, `CodecKind`, …) + buffer leak/reclaim helper impl only — **not** a shared status/error-code enum; `rlib`-only, no C symbols of its own (ADR-0015) |
| Umbrella (optional) | `mediaway-ffi` | One `cdylib` built with **Cargo features** that enable selected `-ffi` crates |

```text
[host language]
    → mediaway-container-ffi.mdylib      (slim: container mux/demux)
    → or mediaway-ffi + features=["container","encoder"]   (one lib, still selective)
        → Rust facades / sans-io / platform crates
[Rust apps] → facades / platform crates directly (preferred)
```

### Feature rules (umbrella)

- **Default features = none** (or documentation-only) — enabling the crate alone must not pull encoders/devices.
- Each feature enables **exactly** the corresponding `-ffi` / platform dep (e.g. `container` → `mediaway-container-ffi`, `encoder-windows` → encoder FFI + windows backend).
- CI builds at least one **slim** feature set (e.g. mux-only) and one fuller set.

### About `mediaway-common`

Depending on `mediaway-common` from an FFI crate is **expected and fine** — it is shared types, kept small. Avoid linking unrelated **capability/platform** crates; that is what per-capability `-ffi` (+ features) prevents.

### Artifacts

| Item | Value |
|------|--------|
| Crate crate-types | `cdylib` + `staticlib` as needed |
| Headers | Per crate (`mediaway_container.h`, …); umbrella may emit an amalgam when features are on |
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

After a Windows-oriented Rust capability is wrappable (likely container first → `mediaway-container-ffi`). Not a scaffold-day deliverable. Each `-ffi` crate gets its own `docs/roadmap.md` when created.

## Out of scope (initially)

- Full API parity with Rust
- Shipping Tier B language packages in the first FFI milestone (C ABI first; wrappers later)
