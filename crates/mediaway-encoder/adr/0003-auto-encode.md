# ADR-0003: Auto encode surface

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (types) + `mediaway-encoder-windows` (session)

## Context

Apps want “push frames → auto-selected path” without hand-rolling MFT/DXGI. Low-level [`VideoEncoder`](../src/video.rs) stays first-class. Facade must not depend on platform crates ([ADR-0002](0002-facade-platform-boundary.md)). Public APIs stay Rust-idiomatic ([`code-style.md`](../../../docs/conventions/code-style.md) § Public Rust API shape).

## Decision

> **Types** in `mediaway_encoder::auto` (`EncodePathClass`, `FallbackPolicy`, `AutoVideoEncodeConfig::new`).  
> **Session** in `mediaway_encoder_windows::auto` via [`AutoVideoEncoder::open`](../../mediaway-encoder-windows/src/auto.rs).

### Selection (Windows `AutoVideoEncoder::open`)

1. `gpu_device = DirectX11` → try Zero-Copy.
2. `gpu_device = DirectX12` + `fallback.allow_gpu_copy()` → bridge via
   `D3d12SharedEncodeBridge` (ADR-0006 of `mediaway-encoder-windows`) to a native
   D3D11 device/texture, then open Zero-Copy on the bridge — labeled `GpuCopy`.
   Any other GPU device kind (Vulkan / Metal / WebGpu), or `DirectX12` without
   `allow_gpu_copy()`, is recorded as `Unsupported` and falls through.
3. Else / on failure of 1–2 → CPU upload if `fallback.allow_cpu_upload()`.
4. Readback / SW: policy bits recognized, but neither has an implemented
   backend (no DX11 staging-texture readback in this crate; `mediaway-sw` is
   still an empty Stage-0 placeholder) — `open` fails honestly with
   `EncodeError::NoBackend` rather than faking either path.

Config takes **explicit** `codec` / `width` / `height` — no `h264_1080p`-style presets.

### Rules

1. No dependency cycle; no default platform pull into the facade.
2. Constructors are associated functions (`Type::open`), not free `open_*`.
3. Path class visible via `path_class()` / labels `zc` · `copy` · `upload` · `readback` · `sw`.
4. Future `mediaway-codec` may re-export constructors.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Facade feature depending on backend | Cargo cycle |
| Free function `auto::open` | C-flavored; prefer `AutoVideoEncoder::open` |
| `h264_1080p` preset on config | Resolution belongs to the app |

## Consequences

### Positive

- Apps: `AutoVideoEncoder::open(&AutoVideoEncodeConfig::new(...))`
- Clear size/codec at the call site

### Negative / Trade-offs

- Session type is per-platform until an umbrella exists

## Selection config renamed (2026-07-31)

`FallbackPolicy` (4 independent bitflags) and `EncodeMode`/`OsMode`/`GpuMode` are
replaced by `AutoVideoEncodeConfig::max_path_class: EncodePathClass` (a ceiling, `Ord`)
and `backend: BackendSelection` — see [ADR-0004](0004-backend-preference.md)'s
2026-07-31 addendum for the full shape and rationale. `path_class()` on the session is
unchanged; `resolved_backend() -> Backend` is new alongside it.

## References

- ADR-0001, ADR-0002, [ADR-0004](0004-backend-preference.md) (preference hierarchy)
- [`api-layers.md`](../../../docs/spec/api-layers.md)
